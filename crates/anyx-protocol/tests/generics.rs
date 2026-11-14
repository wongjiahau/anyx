//! Generic types: `GENERIC (item) RECORD page { … }` declares a record or a
//! choice with type parameters, and `page WITH (item: TEXT)` uses it. A
//! value of a use is written exactly like the record or choice with the
//! arguments in place of the parameters.

mod common;

use anyx_protocol::schema::{Field, Type, TypeKind};
use anyx_protocol::{
    Call, Outcome, Response, Value, decode_request, decode_response, encode_request,
};
use common::{assert_snapshot, diagnostics, parse_files, render, schema};

const SHOP: &str = "PROTOCOL shop VERSION 1,

# A page of a long list, and where the next one starts.
GENERIC (item) RECORD page {
    items: LIST: item,
    next: OPTIONAL: TEXT,
    size: OPTIONAL: INTEGER DEFAULT 20,
},

# What came of an attempt.
GENERIC (value, problem) CHOICE outcome {
    done (value: value),
    failed (problem: problem),
},

RECORD order { number: INTEGER, total: INTEGER },

CHOICE decline { insufficient funds, card expired },

TYPE orders: page WITH (item: order),

METHOD list orders RETURNS (orders: page WITH (item: order)),

METHOD pay (order: INTEGER) RETURNS (outcome: outcome WITH (value: TEXT, problem: decline))
    EXAMPLES [
        pay 7 RETURNS done \"receipt 1\",
        pay 8 RETURNS failed: card expired,
    ],

METHOD pay all (orders: LIST: INTEGER)
    RETURNS (outcomes: page WITH (item: outcome WITH (problem: decline, value: TEXT))),

METHOD history RETURNS (orders: orders),
";

fn order(number: i64, total: i64) -> Value {
    Value::Record(
        [
            ("number".to_string(), Value::Integer(number)),
            ("total".to_string(), Value::Integer(total)),
        ]
        .into(),
    )
}

fn page(items: Vec<Value>) -> Value {
    Value::Record(
        [
            ("items".to_string(), Value::List(items)),
            ("size".to_string(), Value::Integer(20)),
        ]
        .into(),
    )
}

/// The value of the only call of `response`, to a call of `method`.
fn returned(schema: &anyx_protocol::Schema, method: usize, response: &str) -> Value {
    let Response::Outcomes(outcomes) = decode_response(schema, &[method], response).unwrap() else {
        panic!("expected outcomes");
    };
    match &outcomes[..] {
        [Outcome::Value(value)] => value.clone(),
        other => panic!("expected a value, found {other:?}"),
    }
}

#[test]
fn a_generic_type_reads_with_its_parameters_and_each_use_with_its_arguments() {
    let schema = schema(SHOP);
    let page_type = &schema.types[0];
    assert_eq!(page_type.parameters, ["item"]);
    let TypeKind::Record(fields) = &page_type.kind else {
        panic!("expected a record");
    };
    assert_eq!(
        fields[0],
        Field {
            doc: None,
            name: "items".to_string(),
            r#type: Type::List(Box::new(Type::Parameter("item".to_string()))),
            optional: false,
            default: None,
        }
    );
    assert_eq!(fields[2].default, Some(Value::Integer(20)));
    assert_eq!(schema.types[1].parameters, ["value", "problem"]);
    assert_eq!(
        schema.types[4].kind,
        TypeKind::Distinct(Type::Generic(0, vec![Type::Named(2)]))
    );
    // Arguments are kept in the order of the parameters, whatever the order
    // written.
    let pay_all = schema.method("pay all _").unwrap();
    let returns = &pay_all.returns.as_ref().unwrap().r#type;
    assert_eq!(
        returns,
        &Type::Generic(0, vec![Type::Generic(1, vec![Type::Text, Type::Named(3)])])
    );
    assert_eq!(
        schema.type_name(returns),
        "page WITH (item: outcome WITH (value: TEXT, problem: decline))"
    );
}

#[test]
fn a_value_of_a_generic_type_is_written_like_the_type_with_its_arguments() {
    let schema = schema(SHOP);
    let value = returned(
        &schema,
        0,
        "list orders RETURNS { items [{ number 1, total 250 }], next \"2\" }",
    );
    let Value::Record(fields) = &value else {
        panic!("expected a record, found {value:?}");
    };
    assert_eq!(fields["items"], Value::List(vec![order(1, 250)]));
    assert_eq!(fields["next"], Value::Text("2".to_string()));
    assert_eq!(fields["size"], Value::Integer(20));
    let response = anyx_protocol::encode_response(
        &schema,
        &[Call {
            method: 0,
            arguments: vec![],
        }],
        &Response::Outcomes(vec![Outcome::Value(value)]),
    )
    .unwrap();
    assert_eq!(
        response,
        "list orders RETURNS { items [{ number 1, total 250 }], next \"2\", size 20 }\n"
    );
    let nested = returned(
        &schema,
        2,
        "pay all [1, 2] RETURNS { items [done \"receipt 1\", failed: insufficient funds] }",
    );
    assert_eq!(
        nested,
        page(vec![
            Value::Variant {
                shape: "done _".to_string(),
                arguments: vec![Value::Text("receipt 1".to_string())],
            },
            Value::Variant {
                shape: "failed _".to_string(),
                arguments: vec![Value::Variant {
                    shape: "insufficient funds".to_string(),
                    arguments: vec![],
                }],
            },
        ])
    );
    assert_eq!(
        returned(&schema, 3, "history RETURNS { items [] }"),
        page(vec![])
    );
}

#[test]
fn a_value_of_a_generic_type_is_checked_against_its_arguments() {
    let schema = schema(
        "PROTOCOL shop VERSION 1,
GENERIC (item) RECORD page { items: LIST: item },
METHOD sum (numbers: page WITH (item: INTEGER)) RETURNS (sum: INTEGER),
",
    );
    let call = Call {
        method: 0,
        arguments: vec![Value::Record(
            [(
                "items".to_string(),
                Value::List(vec![Value::Integer(1), Value::Integer(2)]),
            )]
            .into(),
        )],
    };
    let request = encode_request(&schema, std::slice::from_ref(&call)).unwrap();
    assert_eq!(request, "sum { items [1, 2] }\n");
    assert_eq!(decode_request(&schema, &request).unwrap(), vec![call]);
    assert_eq!(
        decode_request(&schema, "sum { items [\"1\"] }")
            .unwrap_err()
            .to_string(),
        "1:14: in `numbers` of `sum _`: in `items`: found a string; expected a whole number, as in `3`"
    );
}

#[cfg(feature = "json")]
#[test]
fn a_value_of_a_generic_type_is_the_type_with_its_arguments_in_json() {
    use anyx_protocol::json::{decode_response_json, encode_request_json};
    let schema = schema(SHOP);
    assert_eq!(
        decode_response_json(
            &schema,
            "pay all _",
            "pay all [1] RETURNS { items [failed: card expired] }"
        )
        .unwrap(),
        r#"{"kind":"value","value":{"items":[{"kind":"failed","problem":{"kind":"cardExpired"}}],"size":20}}"#
    );
    assert_eq!(
        decode_response_json(
            &schema,
            "history",
            "history RETURNS { items [{ number 1, total 2 }] }"
        )
        .unwrap(),
        r#"{"kind":"value","value":{"items":[{"number":1,"total":2}],"size":20}}"#
    );
    let schema = super_schema();
    assert_eq!(
        encode_request_json(&schema, "sum _", r#"[{"items": [1, 2]}]"#).unwrap(),
        "sum { items [1, 2] }\n"
    );
    assert_eq!(
        encode_request_json(&schema, "sum _", r#"[{"items": ["1"]}]"#).unwrap_err(),
        "in `numbers`: in \"items\": found \"1\"; expected a whole number"
    );
}

#[cfg(feature = "json")]
fn super_schema() -> anyx_protocol::Schema {
    schema(
        "PROTOCOL shop VERSION 1,
GENERIC (item) RECORD page { items: LIST: item },
METHOD sum (numbers: page WITH (item: INTEGER)) RETURNS (sum: INTEGER),
",
    )
}

#[test]
fn defaults_and_examples_are_checked_against_each_use() {
    let schema = schema(
        "PROTOCOL shop VERSION 1,
GENERIC (item) RECORD page { items: LIST: item, size: OPTIONAL: INTEGER DEFAULT 20 },
RECORD search {
    first: OPTIONAL: page WITH (item: TEXT) DEFAULT { items [\"a\"] },
    pages: OPTIONAL: LIST: page WITH (item: INTEGER) DEFAULT [],
},
METHOD find (search: search) RETURNS (found: page WITH (item: TEXT))
    EXAMPLES [find {} RETURNS { items [\"a\"], size 3 }],
",
    );
    let TypeKind::Record(fields) = &schema.types[1].kind else {
        panic!("expected a record");
    };
    assert_eq!(
        fields[0].default,
        Some(Value::Record(
            [
                (
                    "items".to_string(),
                    Value::List(vec![Value::Text("a".to_string())])
                ),
                ("size".to_string(), Value::Integer(20)),
            ]
            .into()
        ))
    );
    assert_eq!(fields[1].default, Some(Value::List(vec![])));
    assert_eq!(schema.methods[0].examples.len(), 1);
    assert_eq!(
        diagnostics(
            "PROTOCOL shop VERSION 1,
GENERIC (item) RECORD page { items: LIST: item },
RECORD search { first: OPTIONAL: page WITH (item: TEXT) DEFAULT { items [3] } },
METHOD find RETURNS (found: page WITH (item: INTEGER)) EXAMPLES [find RETURNS { items [\"a\"] }],
"
        ),
        "schema:3:74: in the `DEFAULT` of `first`: in `items`: found the number `3`; expected a string, as in `\"text\"`
  RECORD search { first: OPTIONAL: page WITH (item: TEXT) DEFAULT { items [3] } },
                                                                           ^"
    );
    assert_eq!(
        diagnostics(
            "PROTOCOL shop VERSION 1,
GENERIC (item) RECORD page { items: LIST: item },
METHOD find RETURNS (found: page WITH (item: INTEGER)) EXAMPLES [find RETURNS { items [\"a\"] }],
"
        ),
        "schema:3:88: in an example of `find`: in `items`: found a string; expected a whole number, as in `3`
  METHOD find RETURNS (found: page WITH (item: INTEGER)) EXAMPLES [find RETURNS { items [\"a\"] }],
                                                                                         ^^^"
    );
}

#[test]
fn a_public_generic_type_is_shared_by_its_full_path() {
    let files = [
        ("protocol.anyx", "PROTOCOL p VERSION 1,"),
        (
            "common.anyx",
            "PUBLIC GENERIC (item) RECORD page { items: LIST: item }, GENERIC (item) RECORD secret { items: LIST: item },",
        ),
        (
            "files.anyx",
            "RECORD file { path: TEXT }, METHOD list RETURNS (files: common/page WITH (item: file)),",
        ),
    ];
    let schema = parse_files(&files).unwrap_or_else(|found| panic!("{}", render(&files, &found)));
    let list = schema.method("files/list").unwrap();
    assert_eq!(
        schema.type_name(&list.returns.as_ref().unwrap().r#type),
        "common/page WITH (item: files/file)"
    );
    let files = [
        files[0],
        files[1],
        (
            "files.anyx",
            "GENERIC (page) RECORD box { value: page }, METHOD list RETURNS (files: common/secret WITH (item: TEXT)),",
        ),
    ];
    let found = parse_files(&files).unwrap_err();
    assert_eq!(
        render(&files, &found),
        "files.anyx:1:10: found the type parameter `page` of `box`, which has the name of the type `common/page`; expected a name that no type visible here has, since `page` in `box` would then mean either; rename the parameter
  GENERIC (page) RECORD box { value: page }, METHOD list RETURNS (files: common/secret WITH (item: TEXT)),
           ^^^^
files.anyx:1:72: found `common/secret`, which is private to the scope `common`; expected a `PUBLIC` type, or one of the scope `files`; write `PUBLIC` before its definition in `common.anyx` to share it
  GENERIC (page) RECORD box { value: page }, METHOD list RETURNS (files: common/secret WITH (item: TEXT)),
                                                                         ^^^^^^^^^^^^^"
    );
}

#[test]
fn the_guide_explains_generic_and_with() {
    let guide = anyx_protocol::guide(&schema(SHOP), "");
    assert!(
        guide.contains(
            "- A GENERIC RECORD or CHOICE has type parameters, which its fields or
  variants use as types: `GENERIC (item) RECORD page`. A use gives a type
  for each, `page WITH (item: TEXT)`, and its value is written with those types
  in place of the parameters.
"
        ),
        "{guide}"
    );
    assert!(
        guide.contains("GENERIC (value, problem) CHOICE outcome {"),
        "{guide}"
    );
    assert!(guide.contains("    pay 7\n"), "{guide}");
}

#[test]
fn a_generic_declaration_is_formatted_on_one_line_where_it_fits() {
    let formatted = anyx_protocol::format_schema(SHOP).unwrap();
    assert!(
        formatted.contains("GENERIC item RECORD page {\n    items: LIST item,\n"),
        "{formatted}"
    );
    assert!(
        formatted.contains("GENERIC (value, problem) CHOICE outcome { done (value: value), failed (problem: problem) },"),
        "{formatted}"
    );
    assert!(
        formatted.contains("METHOD list orders RETURNS orders: page WITH item: order,"),
        "{formatted}"
    );
}

#[test]
fn mistakes_in_generic_types_are_diagnosed() {
    let cases = [
        (
            "a named type or an error group can't be generic",
            "PROTOCOL a VERSION 1, GENERIC (value) TYPE x: LIST: value,",
        ),
        (
            "a keyword before a use of a generic type needs a `:`",
            "PROTOCOL a VERSION 1, GENERIC (item) RECORD page { items: LIST: item }, METHOD list RETURNS (files: LIST page WITH (item: TEXT)),",
        ),
        (
            "a parameter twice, one with the name of a type, and one unused",
            "PROTOCOL a VERSION 1, RECORD diagnostic { line: INTEGER }, GENERIC (item, item) RECORD page { items: LIST: item }, GENERIC (diagnostic, unused) RECORD other { items: LIST: diagnostic },",
        ),
        (
            "a default on a field whose type holds a parameter",
            "PROTOCOL a VERSION 1, GENERIC (item) RECORD box { value: OPTIONAL: item DEFAULT 3, size: OPTIONAL: INTEGER DEFAULT 3 },",
        ),
        (
            "a parameter with arguments",
            "PROTOCOL a VERSION 1, GENERIC (item) RECORD box { value: item, other: item WITH (x: TEXT) },",
        ),
        (
            "a generic type without its arguments, and arguments for a type that isn't generic",
            "PROTOCOL a VERSION 1, GENERIC (item) RECORD box { value: item }, RECORD r { a: INTEGER }, METHOD a RETURNS (x: box), METHOD b RETURNS (x: r WITH (item: TEXT)),",
        ),
        (
            "unknown, missing and repeated arguments",
            "PROTOCOL a VERSION 1, GENERIC (value, problem) CHOICE outcome { done (value: value), failed (problem: problem) }, METHOD a RETURNS (x: outcome WITH (value: TEXT, problme: TEXT)), METHOD b RETURNS (x: outcome WITH (value: TEXT, value: INTEGER, problem: TEXT)),",
        ),
        (
            "a generic type holding itself through an argument",
            "PROTOCOL a VERSION 1, GENERIC (item) RECORD box { value: item }, RECORD tree { child: box WITH (item: tree) }, GENERIC (item) RECORD chain { value: item, next: OPTIONAL: chain WITH (item: item) },",
        ),
        (
            "a generic type using itself with other arguments, directly or through another",
            "PROTOCOL a VERSION 1, GENERIC (item) RECORD nest { value: item, next: LIST: nest WITH (item: LIST: item) }, GENERIC (a) RECORD first { x: LIST: second WITH (b: LIST: a) }, GENERIC (b) RECORD second { y: LIST: first WITH (a: b) },",
        ),
        (
            "a named type over a use of a generic type that holds it",
            "PROTOCOL a VERSION 1, GENERIC (item) RECORD box { value: item }, TYPE boxed: box WITH (item: boxed),",
        ),
    ];
    let rendered: Vec<String> = cases
        .iter()
        .map(|(title, source)| format!("== {title}\n{source}\n{}\n", diagnostics(source)))
        .collect();
    assert_snapshot(
        "tests/snapshots/generic_diagnostics.txt",
        &rendered.join("\n"),
    );
}

#[test]
fn a_generic_type_may_hold_itself_through_a_list_with_its_own_parameters() {
    let schema = schema(
        "PROTOCOL a VERSION 1,
GENERIC (item) RECORD tree { value: item, children: LIST: tree WITH (item: item) },
METHOD tree RETURNS (tree: tree WITH (item: TEXT)),
",
    );
    let value = returned(
        &schema,
        0,
        "tree RETURNS { value \"a\", children [{ value \"b\", children [] }] }",
    );
    let Value::Record(fields) = value else {
        panic!("expected a record");
    };
    assert_eq!(fields["value"], Value::Text("a".to_string()));
}

#[test]
fn the_generic_example_in_protocol_md_reads() {
    let document = common::read("PROTOCOL.md");
    let example = document
        .split("```anyx\n")
        .find(|block| block.contains("GENERIC (item) RECORD page"))
        .and_then(|rest| rest.split("```").next())
        .unwrap();
    let schema = schema(&format!(
        "PROTOCOL shop VERSION 1,
RECORD order {{ number: INTEGER, total: INTEGER }},
TYPE order id: INTEGER,
RECORD receipt {{ number: INTEGER }},
CHOICE decline {{ insufficient funds, card expired }},
{example}"
    ));
    let pay_all = schema.method("pay all _").unwrap();
    assert_eq!(
        schema.type_name(&pay_all.returns.as_ref().unwrap().r#type),
        "page WITH (item: outcome WITH (value: receipt, problem: decline))"
    );
}
