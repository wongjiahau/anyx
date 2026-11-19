//! Generic types: `RECORD page of (item: Type) { … }` declares a record or a
//! choice whose name is a shape with a type parameter in each slot, and
//! `page of (Text)` uses it, with a type in each slot. A slot may sit
//! anywhere among the words. A value of a use is written exactly like the
//! record or choice with the arguments in place of the parameters.

mod common;

use anyx_protocol::schema::{Field, Type, TypeKind};
use anyx_protocol::{
    Call, Outcome, Response, Value, decode_request, decode_response, encode_request,
};
use common::{assert_snapshot, diagnostics, parse_files, render, schema};

const SHOP: &str = "PROTOCOL shop VERSION 1,

# A page of a long list, and where the next one starts.
RECORD page of (item: Type) {
    items: LIST: item,
    next: Text DEFAULT \"\",
    size: Integer DEFAULT 20,
},

# What came of an attempt.
CHOICE (value: Type) or (problem: Type) {
    done (value: value),
    failed (problem: problem),
},

RECORD Order { number: Integer, total: Integer },

CHOICE Decline { insufficient funds, card expired },

TYPE Orders: page of (Order),

METHOD list orders RETURNS (orders: page of (Order)),

METHOD pay (order: Integer) RETURNS (outcome: (Text) or (Decline))
    EXAMPLES [
        pay 7 RETURNS done \"receipt 1\",
        pay 8 RETURNS failed: card expired,
    ],

METHOD pay all (orders: LIST Integer)
    RETURNS (outcomes: page of ((Text) or (Decline))),

METHOD history RETURNS (orders: Orders),
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
            ("next".to_string(), Value::Text(String::new())),
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
    assert_eq!(page_type.name, "page of _");
    assert_eq!(page_type.shape(), "page of (item)");
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
            default: None,
            form: anyx_protocol::schema::FieldForm::Shape("items _".to_string()),
        }
    );
    assert_eq!(fields[2].default, Some(Value::Integer(20)));
    assert_eq!(schema.types[1].name, "_ or _");
    assert_eq!(schema.types[1].parameters, ["value", "problem"]);
    assert_eq!(
        schema.types[4].kind,
        TypeKind::Distinct(Type::Generic(0, vec![Type::Named(2)]))
    );
    // Arguments are matched to the parameters by their slots.
    let pay_all = schema.method("pay all _").unwrap();
    let returns = &pay_all.returns.as_ref().unwrap().r#type;
    assert_eq!(
        returns,
        &Type::Generic(0, vec![Type::Generic(1, vec![Type::Text, Type::Named(3)])])
    );
    assert_eq!(schema.type_name(returns), "page of ((Text) or (Decline))");
}

#[test]
fn a_value_of_a_generic_type_is_written_like_the_type_with_its_arguments() {
    let schema = schema(SHOP);
    let value = returned(
        &schema,
        0,
        "RETURNS { items [{ number 1, total 250 }], next \"2\" }",
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
        "RETURNS { items [{ number 1, total 250 }], next \"2\", size 20 }\n"
    );
    let nested = returned(
        &schema,
        2,
        "RETURNS { items [done \"receipt 1\", failed: insufficient funds] }",
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
    assert_eq!(returned(&schema, 3, "RETURNS { items [] }"), page(vec![]));
}

#[test]
fn a_value_of_a_generic_type_is_checked_against_its_arguments() {
    let schema = schema(
        "PROTOCOL shop VERSION 1,
RECORD page of (item: Type) { items: LIST: item },
METHOD sum (numbers: page of (Integer)) RETURNS (sum: Integer),
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
    assert_eq!(request, "sum ({ items [1, 2] } AS page of (Integer))\n");
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
            "RETURNS { items [failed: card expired] }"
        )
        .unwrap(),
        r#"{"kind":"value","value":{"items":[{"kind":"failed","problem":{"kind":"cardExpired"}}],"next":"","size":20}}"#
    );
    assert_eq!(
        decode_response_json(
            &schema,
            "history",
            "RETURNS { items [{ number 1, total 2 }] }"
        )
        .unwrap(),
        r#"{"kind":"value","value":{"items":[{"number":1,"total":2}],"next":"","size":20}}"#
    );
    let schema = super_schema();
    assert_eq!(
        encode_request_json(&schema, "sum _", r#"[{"items": [1, 2]}]"#).unwrap(),
        "sum ({ items [1, 2] } AS page of (Integer))\n"
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
RECORD page of (item: Type) { items: LIST: item },
METHOD sum (numbers: page of (Integer)) RETURNS (sum: Integer),
",
    )
}

#[test]
fn defaults_and_examples_are_checked_against_each_use() {
    let schema = schema(
        "PROTOCOL shop VERSION 1,
RECORD page of (item: Type) { items: LIST: item, size: Integer DEFAULT 20 },
RECORD Search {
    first: page of (Text) DEFAULT { items [\"a\"] },
    pages: LIST page of (Integer) DEFAULT [],
},
METHOD find (search: Search) RETURNS (found: page of (Text))
    EXAMPLES [find ({} AS: Search) RETURNS { items [\"a\"], size 3 }],
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
RECORD page of (item: Type) { items: LIST: item },
RECORD Search { first: page of (Text) DEFAULT { items [3] } },
METHOD find RETURNS (found: page of (Integer)) EXAMPLES [find RETURNS { items [\"a\"] }],
"
        ),
        "schema:3:56: in the `DEFAULT` of `first`: in `items`: found the number `3`; expected a string, as in `\"text\"`
  RECORD Search { first: page of (Text) DEFAULT { items [3] } },
                                                         ^"
    );
    assert_eq!(
        diagnostics(
            "PROTOCOL shop VERSION 1,
RECORD page of (item: Type) { items: LIST: item },
METHOD find RETURNS (found: page of (Integer)) EXAMPLES [find RETURNS { items [\"a\"] }],
"
        ),
        "schema:3:80: in an example of `find`: in `items`: found a string; expected a whole number, as in `3`
  METHOD find RETURNS (found: page of (Integer)) EXAMPLES [find RETURNS { items [\"a\"] }],
                                                                                 ^^^"
    );
}

#[test]
fn a_public_generic_type_is_shared_by_a_direct_import() {
    let files = [
        (
            "protocol.protocol.anyx",
            "PROTOCOL p VERSION 1, USE \"./files.protocol.anyx\",",
        ),
        (
            "common.protocol.anyx",
            "PUBLIC RECORD page of (item: Type) { items: LIST: item }, RECORD secret of (item: Type) { items: LIST: item },",
        ),
        (
            "files.protocol.anyx",
            "USE \"./common.protocol.anyx\", RECORD File { path: Text }, METHOD list RETURNS (files: page of (File)),",
        ),
    ];
    let schema = parse_files(&files).unwrap_or_else(|found| panic!("{}", render(&files, &found)));
    let list = schema.method("list").unwrap();
    assert_eq!(
        schema.type_name(&list.returns.as_ref().unwrap().r#type),
        "page of (File)"
    );
    let files = [
        files[0],
        files[1],
        (
            "files.protocol.anyx",
            "USE \"./common.protocol.anyx\", RECORD (page: Type) boxed { value: page }, METHOD list RETURNS (files: secret of (Text)), METHOD all RETURNS (files: page (Text)),",
        ),
    ];
    let found = parse_files(&files).unwrap_err();
    assert_snapshot(
        "tests/snapshots/generic_scope_diagnostics.txt",
        &render(&files, &found),
    );
}

/// A slot of a type's shape may sit anywhere: after the words, before them,
/// between two words, or around them, and a use fills each in place.
const PLACES: &str = "PROTOCOL places VERSION 1,

RECORD Person { name: Text },
RECORD Document { title: Text },

## The words, then a slot.
RECORD page of (item: Type) { items: LIST: item },
## A slot, then the words.
RECORD (item: Type) history { versions: LIST: item },
## A slot, a word, a slot.
CHOICE (left: Type) or (right: Type) { first (value: left), second (value: right) },
## Slots between words.
RECORD table from (key: Type) to (value: Type) { keys: LIST: key, rows: LIST: value },
## A slot first, then words, then a slot.
RECORD (value: Type) signed by (signer: Type) { value: value, signer: signer },
## A slot between words, with words after it.
RECORD box of (item: Type) sealed { item: item },
## The same words as a type that isn't generic.
RECORD Page { number: Integer },

METHOD list RETURNS (people: page of (Person)),
METHOD history RETURNS (history: (Document) history),
METHOD parse (input: Text) RETURNS (parsed: (Integer) or (Text)),
METHOD table RETURNS (table: table from (Text) to (Person)),
METHOD search (query: Text) RETURNS (results: page of ((Document) signed by (Person))),
METHOD sealed RETURNS (box: box of (LIST Text) sealed),
METHOD first page RETURNS (page: Page),
METHOD nested RETURNS (pages: LIST page of ((page of (Integer)) or (MAP: (Text) history))),
";

#[test]
fn a_slot_may_sit_anywhere_in_a_generic_type_s_shape() {
    let schema = schema(PLACES);
    let declared: Vec<(&str, String, &[String])> = schema
        .types
        .iter()
        .map(|definition| {
            (
                definition.name.as_str(),
                definition.shape(),
                definition.parameters.as_slice(),
            )
        })
        .collect();
    let expected: Vec<(&str, &str, Vec<&str>)> = vec![
        ("Person", "Person", vec![]),
        ("Document", "Document", vec![]),
        ("page of _", "page of (item)", vec!["item"]),
        ("_ history", "(item) history", vec!["item"]),
        ("_ or _", "(left) or (right)", vec!["left", "right"]),
        (
            "table from _ to _",
            "table from (key) to (value)",
            vec!["key", "value"],
        ),
        (
            "_ signed by _",
            "(value) signed by (signer)",
            vec!["value", "signer"],
        ),
        ("box of _ sealed", "box of (item) sealed", vec!["item"]),
        ("Page", "Page", vec![]),
        ("Boolean", "Boolean", vec![]),
        ("_ ?", "(item) ?", vec!["item"]),
        ("Schema File", "Schema File", vec![]),
    ];
    assert_eq!(declared.len(), expected.len());
    for ((name, shape, parameters), (expected_name, expected_shape, expected_parameters)) in
        declared.iter().zip(&expected)
    {
        assert_eq!(name, expected_name);
        assert_eq!(shape, expected_shape);
        assert_eq!(parameters, expected_parameters);
    }
    let returned: Vec<String> = schema
        .methods
        .iter()
        .map(|method| schema.type_name(&method.returns.as_ref().unwrap().r#type))
        .collect();
    assert_eq!(
        returned,
        [
            "page of (Person)",
            "(Document) history",
            "(Integer) or (Text)",
            "table from (Text) to (Person)",
            "page of ((Document) signed by (Person))",
            "box of (LIST Text) sealed",
            "Page",
            "LIST page of ((page of (Integer)) or (MAP (Text) history))",
        ]
    );
    // Each argument goes to the parameter of its slot.
    let table = &schema
        .method("table")
        .unwrap()
        .returns
        .as_ref()
        .unwrap()
        .r#type;
    assert_eq!(table, &Type::Generic(5, vec![Type::Text, Type::Named(0)]));
    let search = &schema
        .method("search _")
        .unwrap()
        .returns
        .as_ref()
        .unwrap()
        .r#type;
    assert_eq!(
        search,
        &Type::Generic(
            2,
            vec![Type::Generic(6, vec![Type::Named(1), Type::Named(0)])]
        )
    );
}

#[test]
fn a_value_of_each_placement_is_written_like_its_type_with_the_arguments() {
    let schema = schema(PLACES);
    let method = |name: &str| {
        schema
            .methods
            .iter()
            .position(|method| method.full_name() == name)
            .unwrap()
    };
    let person =
        |name: &str| Value::Record([("name".to_string(), Value::Text(name.to_string()))].into());
    assert_eq!(
        returned(
            &schema,
            method("list"),
            "RETURNS { items [{ name \"Ann\" }] }"
        ),
        Value::Record([("items".to_string(), Value::List(vec![person("Ann")]))].into())
    );
    assert_eq!(
        returned(
            &schema,
            method("history"),
            "RETURNS { versions [{ title \"a\" }] }"
        ),
        Value::Record(
            [(
                "versions".to_string(),
                Value::List(vec![Value::Record(
                    [("title".to_string(), Value::Text("a".to_string()))].into()
                )])
            )]
            .into()
        )
    );
    assert_eq!(
        returned(&schema, method("parse _"), "RETURNS second \"x\""),
        Value::Variant {
            shape: "second _".to_string(),
            arguments: vec![Value::Text("x".to_string())],
        }
    );
    assert_eq!(
        decode_response(&schema, &[method("parse _")], "RETURNS first \"x\"")
            .unwrap_err()
            .to_string(),
        "1:15: in `value` of `first _`: found a string; expected a whole number, as in `3`"
    );
    assert_eq!(
        returned(
            &schema,
            method("table"),
            "RETURNS { keys [\"a\"], rows [{ name \"Ann\" }] }"
        ),
        Value::Record(
            [
                (
                    "keys".to_string(),
                    Value::List(vec![Value::Text("a".to_string())])
                ),
                ("rows".to_string(), Value::List(vec![person("Ann")])),
            ]
            .into()
        )
    );
    let signed = returned(
        &schema,
        method("search _"),
        "RETURNS { items [{ value { title \"a\" }, signer { name \"Ann\" } }] }",
    );
    let Value::Record(fields) = signed else {
        panic!("expected a record");
    };
    let Value::List(items) = &fields["items"] else {
        panic!("expected a list");
    };
    let Value::Record(item) = &items[0] else {
        panic!("expected a record");
    };
    assert_eq!(item["signer"], person("Ann"));
    assert_eq!(
        returned(&schema, method("sealed"), "RETURNS { item [\"a\"] }"),
        Value::Record(
            [(
                "item".to_string(),
                Value::List(vec![Value::Text("a".to_string())])
            )]
            .into()
        )
    );
    assert_eq!(
        returned(&schema, method("first page"), "RETURNS { number 1 }"),
        Value::Record([("number".to_string(), Value::Integer(1))].into())
    );
    assert_eq!(
        returned(
            &schema,
            method("nested"),
            "RETURNS [{ items [first { items [1] }, second { \"k\" { versions [\"v\"] } }] }]"
        ),
        Value::List(vec![Value::Record(
            [(
                "items".to_string(),
                Value::List(vec![
                    Value::Variant {
                        shape: "first _".to_string(),
                        arguments: vec![Value::Record(
                            [("items".to_string(), Value::List(vec![Value::Integer(1)]))].into()
                        )],
                    },
                    Value::Variant {
                        shape: "second _".to_string(),
                        arguments: vec![Value::Map(
                            [(
                                "k".to_string(),
                                Value::Record(
                                    [(
                                        "versions".to_string(),
                                        Value::List(vec![Value::Text("v".to_string())])
                                    )]
                                    .into()
                                )
                            )]
                            .into()
                        )],
                    },
                ])
            )]
            .into()
        )])
    );
}

#[test]
fn the_guide_explains_generic_types() {
    let guide = anyx_protocol::guide(&schema(SHOP), "");
    assert!(
        guide.contains(
            "- A generic RECORD or CHOICE has a type parameter in each slot of its
  name, which its fields or variants use as a type: `RECORD page of (item: Type)`.
  A use puts a type in each slot, in parentheses, `page of (Text)`, and its value
  is written with those types in place of the parameters.
"
        ),
        "{guide}"
    );
    assert!(
        guide.contains("CHOICE (value: Type) or (problem: Type) {"),
        "{guide}"
    );
    assert!(guide.contains("    pay 7\n"), "{guide}");
}

#[test]
fn a_generic_declaration_is_formatted_with_its_slots_in_place() {
    let formatted = anyx_protocol::format_schema(SHOP).unwrap();
    assert!(
        formatted.contains("RECORD page of (item: Type) { items: LIST item, "),
        "{formatted}"
    );
    assert!(
        formatted.contains("CHOICE (value: Type) or (problem: Type) { done (value: value), failed (problem: problem) },"),
        "{formatted}"
    );
    assert!(
        formatted.contains("METHOD list orders RETURNS orders: page of (Order),"),
        "{formatted}"
    );
    assert!(
        formatted.contains("RETURNS outcomes: page of ((Text) or (Decline)),"),
        "{formatted}"
    );
    // Formatting changes nothing a schema means.
    let formatted = anyx_protocol::format_schema(PLACES).unwrap();
    let before = schema(PLACES);
    let after = schema(&formatted);
    assert_eq!(before.types, after.types);
    assert_eq!(before.methods, after.methods);
}

#[test]
fn mistakes_in_generic_types_are_diagnosed() {
    let cases = [
        (
            "a named type can't be generic",
            "PROTOCOL a VERSION 1, TYPE list of (value: Type): LIST: value,",
        ),
        (
            "a slot of a declaration holds a type parameter, not a type",
            "PROTOCOL a VERSION 1, RECORD page of (item: Text) { items: LIST: item },",
        ),
        (
            "an error group can't be generic",
            "PROTOCOL a VERSION 1, ERRORS failures of (item: Type) [missing (item: item)],",
        ),
        (
            "the braces come last, once, and the name has words",
            "PROTOCOL a VERSION 1, RECORD page of (item: Type), RECORD box { value: Text } of (item: Type) { value: item }, RECORD (left: Type) (right: Type) { left: left, right: right },",
        ),
        (
            "a shape twice, while the same words without the slot are another type",
            "PROTOCOL a VERSION 1, RECORD Page { number: Integer }, RECORD Page of (item: Type) { items: LIST: item }, CHOICE Page of (other: Type) { one (value: other) },",
        ),
        (
            "a parameter twice and an unused parameter",
            "PROTOCOL a VERSION 1, RECORD (item: Type) paired with (item: Type) { items: LIST: item }, RECORD other of (diagnostic: Type) and (unused: Type) { items: LIST: diagnostic },",
        ),
        (
            "a default on a field whose type holds a parameter",
            "PROTOCOL a VERSION 1, RECORD box of (item: Type) { value: item DEFAULT 3, size: Integer DEFAULT 3 },",
        ),
        (
            "a parameter with a type in a slot",
            "PROTOCOL a VERSION 1, RECORD box of (item: Type) { value: item, other: item of (Text) },",
        ),
        (
            "a generic type without its arguments, and an argument for a type that isn't generic",
            "PROTOCOL a VERSION 1, RECORD box of (item: Type) { value: item }, RECORD Row { a: Integer }, METHOD a RETURNS (x: box), METHOD b RETURNS (x: Row (Text)),",
        ),
        (
            "the wrong number of arguments, or the slots in the wrong places",
            "PROTOCOL a VERSION 1, CHOICE (value: Type) or (problem: Type) { done (value: value), failed (problem: problem) }, METHOD a RETURNS (x: (Text) or (Text) (Integer)), METHOD b RETURNS (x: or (Text) (Integer)), METHOD c RETURNS (x: (Text) or),",
        ),
        (
            "a type argument that names no type",
            "PROTOCOL a VERSION 1, RECORD page of (item: Type) { items: LIST: item }, METHOD a RETURNS (x: page of (prson)),",
        ),
        (
            "a generic type holding itself through an argument",
            "PROTOCOL a VERSION 1, RECORD box of (item: Type) { value: item }, RECORD Tree { child: box of (Tree) }, RECORD (item: Type) chain { value: item, next: ((item) chain)? DEFAULT null },",
        ),
        (
            "a generic type using itself with other arguments, directly or through another",
            "PROTOCOL a VERSION 1, RECORD nest of (item: Type) { value: item, next: LIST: nest of (LIST: item) }, RECORD (a: Type) first { x: LIST: second (LIST: a) }, RECORD second (b: Type) { y: LIST: (b) first },",
        ),
        (
            "a named type over a use of a generic type that holds it",
            "PROTOCOL a VERSION 1, RECORD box of (item: Type) { value: item }, TYPE Boxed: box of (Boxed),",
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
RECORD tree of (item: Type) { value: item, children: LIST: tree of (item) },
RECORD (item: Type) chain { value: item, rest: MAP: (item) chain },
METHOD tree RETURNS (tree: tree of (Text)),
METHOD chain RETURNS (chain: (Integer) chain),
",
    );
    let value = returned(
        &schema,
        0,
        "RETURNS { value \"a\", children [{ value \"b\", children [] }] }",
    );
    let Value::Record(fields) = value else {
        panic!("expected a record");
    };
    assert_eq!(fields["value"], Value::Text("a".to_string()));
    let value = returned(
        &schema,
        1,
        "RETURNS { value 1, rest { \"next\" { value 2, rest {} } } }",
    );
    let Value::Record(fields) = value else {
        panic!("expected a record");
    };
    assert_eq!(fields["value"], Value::Integer(1));
}

#[test]
fn the_generic_example_in_protocol_md_reads() {
    let document = common::read("PROTOCOL.md");
    let example = document
        .split("```anyx\n")
        .find(|block| block.contains("RECORD Page of (item: Type)"))
        .and_then(|rest| rest.split("```").next())
        .unwrap();
    let schema = schema(&format!(
        "PROTOCOL shop VERSION 1,
RECORD Order {{ number: Integer, total: Integer }},
TYPE Order Id: Integer,
RECORD Receipt {{ number: Integer }},
RECORD Document {{ title: Text }},
RECORD Person {{ name: Text }},
CHOICE Decline {{ insufficient funds, card expired }},
{example}"
    ));
    let pay_all = schema.method("pay all _").unwrap();
    assert_eq!(
        schema.type_name(&pay_all.returns.as_ref().unwrap().r#type),
        "Page of ((Receipt) or (Decline))"
    );
    let search = schema.method("search _").unwrap();
    assert_eq!(
        schema.type_name(&search.returns.as_ref().unwrap().r#type),
        "Page of ((Document) signed by (Person))"
    );
}
