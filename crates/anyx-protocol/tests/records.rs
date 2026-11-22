//! Record field shapes, defaults, and variant-based entries.
mod common;

use anyx_protocol::{Value, decode_request, encode_request};
use common::{diagnostics, schema};

const SEARCH: &str = "PROTOCOL a VERSION 1,
TYPE ALIAS Case [| `smart, `any, `exact |],
TYPE ALIAS Order [| `newest first, `by name, `largest first |],
TYPE ALIAS Auth [| `bearer (token: Text), `no auth |],
TYPE ALIAS Search: {
    # The pattern to look for.
    pattern: Text,
    (Case) case DEFAULT (`smart) case,
    # Searches hidden files too.
    `(hidden: [| `hidden, `null |]) DEFAULT `null,
    `(pinned: [| `pinned, `null |]) DEFAULT `null,
    max depth (Integer?) DEFAULT max depth (`null),
    at least (Integer?) bytes DEFAULT at least (`null) bytes,
    `(order: Order) DEFAULT `newest first,
    `(auth: Auth) DEFAULT `no auth,
},
METHOD find (search: Search) RETURNS (count: Integer) ERRORS [| |],";

fn find(source: &str) -> Value {
    let schema = schema(SEARCH);
    let calls = decode_request(&schema, source).unwrap_or_else(|error| panic!("{error:?}"));
    calls[0].arguments[0].clone()
}

fn field<'v>(record: &'v Value, name: &str) -> &'v Value {
    let Value::Record(fields) = record else {
        panic!("expected a record, found {record:?}")
    };
    &fields[name]
}

fn variant(shape: &str, arguments: Vec<Value>) -> Value {
    Value::Variant {
        shape: shape.to_string(),
        arguments,
    }
}

#[test]
fn each_entry_finds_its_field_by_its_shape() {
    let search = find(
        "find { (`any) case, `by name, pattern \"ode\", at least 14 bytes, `hidden, `bearer \"t\", max depth 3 }",
    );
    assert_eq!(field(&search, "pattern"), &Value::Text("ode".into()));
    assert_eq!(field(&search, "case"), &variant("any", vec![]));
    assert_eq!(field(&search, "hidden"), &variant("hidden", vec![]));
    assert_eq!(field(&search, "max depth"), &Value::Integer(3));
    assert_eq!(field(&search, "at least bytes"), &Value::Integer(14));
    assert_eq!(field(&search, "order"), &variant("by name", vec![]));
    assert_eq!(
        field(&search, "auth"),
        &variant("bearer _", vec![Value::Text("t".into())])
    );
}

#[test]
fn fields_left_out_take_their_defaults() {
    let search = find("find { pattern \"ode\" }");
    assert_eq!(field(&search, "case"), &variant("smart", vec![]));
    assert_eq!(field(&search, "hidden"), &variant("null", vec![]));
    assert_eq!(field(&search, "max depth"), &variant("null", vec![]));
    assert_eq!(field(&search, "order"), &variant("newest first", vec![]));
    assert_eq!(field(&search, "auth"), &variant("no auth", vec![]));
}

#[test]
fn a_record_is_written_back_as_a_reader_writes_it() {
    let schema = schema(SEARCH);
    for (source, written) in [
        (
            "find { pattern \"ode\" }",
            "find { pattern \"ode\", (`smart) case }\n",
        ),
        (
            "find { pattern \"ode\", (`any) case, `hidden, max depth 3, at least 14 bytes, `by name, `bearer \"t\" }",
            "find { pattern \"ode\", (`any) case, `hidden, max depth 3, at least 14 bytes, `by name, `bearer \"t\" }\n",
        ),
    ] {
        let calls = decode_request(&schema, source).unwrap();
        let encoded = encode_request(&schema, &calls).unwrap();
        assert_eq!(encoded, written);
        assert_eq!(decode_request(&schema, &encoded).unwrap(), calls);
    }
}

#[test]
fn an_entry_no_field_takes_is_reported_with_the_fields() {
    let schema = schema(SEARCH);
    for source in [
        "find { pattern \"ode\", `exact }",
        "find { pattern \"ode\", `any case }",
        "find { pattern \"ode\", \"x\" }",
    ] {
        let error = decode_request(&schema, source).unwrap_err();
        assert!(
            format!("{error:?}")
                .contains("expected a field: `pattern …`, `… case`, a variant for `hidden`"),
            "{source}: {error:?}"
        );
    }
}

#[test]
fn a_value_before_words_without_parentheses_gets_a_hint() {
    let error = decode_request(&schema(SEARCH), "find { pattern \"ode\", `any case }").unwrap_err();
    assert!(
        format!("{error:?}")
            .contains("a value before a field's words goes in parentheses: `` (`any) case ``"),
        "{error:?}"
    );
}

#[test]
fn two_fields_written_as_their_variant_may_not_share_one() {
    let found = diagnostics(
        "PROTOCOL a VERSION 1,
        TYPE ALIAS Order [| `by (key: Text), `newest first |],
        TYPE ALIAS Sort [| `by (rank: Integer) |],
        TYPE ALIAS Rec: { `(order: Order), `(sort: Sort) },",
    );
    assert!(
        found.contains("found `` `by _ `` as a value of both fields `order` and `sort`"),
        "{found}"
    );
}

#[test]
fn a_field_written_as_its_variant_holds_only_variants() {
    for (field, r#type) in [
        ("`(count: Integer)", "`Integer`"),
        ("`(path: Text)", "`Text`"),
        ("`(wait: Duration)", "`Duration`"),
        ("`(paths: LIST Text)", "`LIST Text`"),
        ("`(count: Integer?)", "`Integer?`"),
    ] {
        let source = format!("PROTOCOL a VERSION 1, TYPE ALIAS Rec: {{ {field} }},");
        let found = diagnostics(&source);
        assert!(
            found.contains(&format!(
                "of {type}; expected a variant or a union of variants, since the field is written as its variant alone"
            )),
            "{field}: {found}"
        );
    }
    // A choice, a union of variants, and either made nullable.
    let schema = schema(
        "PROTOCOL a VERSION 1,
        TYPE ALIAS Order [| `newest first, `by name |],
        TYPE ALIAS Rec: { `(order: Order?), `(mode: [| `fast, `slow |]) },
        METHOD find (rec: Rec) RETURNS (count: Integer) ERRORS [| |],",
    );
    let calls = decode_request(&schema, "find { `slow, `null }").unwrap();
    assert_eq!(
        field(&calls[0].arguments[0], "mode"),
        &variant("slow", vec![])
    );
    assert_eq!(
        field(&calls[0].arguments[0], "order"),
        &variant("null", vec![])
    );
}

#[test]
fn mistakes_in_fields_are_diagnosed() {
    for (fields, message) in [
        (
            "quiet from (Integer) to (end: Integer)",
            "found an unnamed slot; write `(name: Type)`",
        ),
        (
            "(edge: Integer) by (edge: Integer)",
            "found a second slot named `edge`",
        ),
        (
            "quiet from (start: Integer) to (end: Integer), quiet: Integer",
            "found a second field named `quiet`",
        ),
        (
            "between (start: Integer) and (end: Integer) DEFAULT between 1",
            "found the call `between _`; expected an entry of the field, as a value writes it: `between … and …`",
        ),
        (
            "age (Integer) DEFAULT years 0",
            "found the call `years _`; expected an entry of the field, as a value writes it: `age …`",
        ),
        (
            "age: Integer DEFAULT age 0",
            "found a `DEFAULT` in a field's slot; expected it after the whole field",
        ),
        (
            "hidden DEFAULT hidden",
            "write `` `(hidden: [| `hidden, `null |]) DEFAULT `null ``",
        ),
        (
            "age (Integer) DEFAULT age 0, age (Integer)",
            "found a second field named `age`",
        ),
    ] {
        let source = format!("PROTOCOL a VERSION 1, TYPE ALIAS Rec: {{ {fields} }},");
        let found = diagnostics(&source);
        assert!(found.contains(message), "{fields}: {found}");
    }
}

#[test]
fn a_field_written_as_its_variant_of_a_type_parameter_is_diagnosed() {
    let found =
        diagnostics("PROTOCOL a VERSION 1, TYPE ALIAS Box of (Item: TYPE): { `(value: Item) },");
    assert!(
        found.contains(
            "found the field `value` of `Item`; expected a variant or a union of variants"
        ),
        "{found}"
    );
}

const RANGE: &str = "PROTOCOL a VERSION 1,
TYPE ALIAS Depth: Integer . WHERE [0 <= _],
TYPE ALIAS Search: {
    depth from (start: Depth) to (end: Depth?),
    between (start: Integer) and (end: Integer) bytes DEFAULT between 1 and 9 bytes,
},
METHOD find (search: Search) RETURNS (count: Integer) ERRORS [| |],";

#[test]
fn a_field_with_several_slots_holds_a_value_for_each() {
    let schema = schema(RANGE);
    let calls = decode_request(&schema, "find { depth from 1 to 3 }").unwrap();
    let search = &calls[0].arguments[0];
    assert_eq!(
        field(search, "depth"),
        &Value::Tuple(vec![Value::Integer(1), Value::Integer(3)])
    );
    assert_eq!(
        field(search, "between"),
        &Value::Tuple(vec![Value::Integer(1), Value::Integer(9)])
    );
    let encoded = encode_request(&schema, &calls).unwrap();
    assert_eq!(
        encoded,
        "find { depth from 1 to 3, between 1 and 9 bytes }\n"
    );
    assert_eq!(decode_request(&schema, &encoded).unwrap(), calls);
}

#[test]
fn each_slot_is_checked_against_its_own_type() {
    let schema = schema(RANGE);
    let error = decode_request(&schema, "find { depth from -1 to 3 }").unwrap_err();
    assert!(
        format!("{error:?}").contains("in `depth`: value `-1` is not a `(Integer) WHERE [0 <= _]`"),
        "{error:?}"
    );
}

#[cfg(feature = "json")]
#[test]
fn a_field_with_several_slots_uses_named_json_properties() {
    use anyx_protocol::json::{decode_request_json, encode_request_json};
    let schema = schema(RANGE);
    let json = decode_request_json(&schema, "find { depth from 1 to (`null) }").unwrap();
    assert!(
        json.contains(r#""depth":{"end":{"kind":"null"},"start":1}"#),
        "{json}"
    );
    let arguments =
        r#"[{"depth": {"end": {"kind": "null"}, "start": 1}, "between": {"start": 1, "end": 9}}]"#;
    assert_eq!(
        encode_request_json(&schema, "find _", arguments).unwrap(),
        "find { depth from 1 to (`null), between 1 and 9 bytes }\n"
    );
}

#[test]
fn a_shared_default_variant_must_be_left_out() {
    let schema = schema(SEARCH);
    let error = decode_request(&schema, "find { pattern \"ode\", `null }").unwrap_err();
    assert!(
        error
            .to_string()
            .contains("could fill more than one unnamed field (`hidden`, `pinned`): leave it out"),
        "{error:?}"
    );
}

#[test]
fn all_fields_that_share_a_variant_must_default_to_it() {
    let found = diagnostics(
        "PROTOCOL a VERSION 1,
        TYPE ALIAS Rec: {
            `(hidden: [| `hidden, `null |]) DEFAULT `null,
            `(pinned: [| `pinned, `null |]) DEFAULT `pinned,
        },",
    );
    assert!(found.contains("be the `DEFAULT` of both fields"), "{found}");
}

#[test]
fn a_variant_with_slots_cannot_be_shared_even_with_matching_defaults() {
    let found = diagnostics(
        "PROTOCOL a VERSION 1,
        TYPE ALIAS Rec: {
            `(first: [| `at (index: Integer) |]) DEFAULT `at 0,
            `(second: [| `at (index: Integer) |]) DEFAULT `at 0,
        },",
    );
    assert!(
        found.contains("expected a shared variant to have no slots"),
        "{found}"
    );
}

#[test]
fn a_flag_type_requires_an_explicit_union() {
    let source = "PROTOCOL a VERSION 1, TYPE ALIAS Rec: { `(hidden: (`hidden)?) DEFAULT `null },";
    assert!(!diagnostics(source).is_empty());
}

#[test]
fn inline_records_work_in_aliases_lists_and_unions() {
    let schema = schema(
        "PROTOCOL inline VERSION 1,
        TYPE ALIAS Page of (Item: TYPE): { rows: LIST { value: Item } },
        TYPE ALIAS Tree: { children: LIST { child: Tree } },
        METHOD search (filter: { limit (Integer) DEFAULT limit 3 }) RETURNS NOTHING ERRORS [| |],
        METHOD submit (page: Page of (Text)) RETURNS NOTHING ERRORS [| `invalid |],
        METHOD save (tree: Tree) RETURNS NOTHING ERRORS [| |],
        METHOD choose (item: [| { count: Integer }, `none |]) RETURNS NOTHING ERRORS [| |],",
    );
    let source = "search {}, submit ({ rows [{ value \"one\" }] } AS: Page of (Text)),
                  save { children [{ child { children [] } }] },
                  choose { count 7 }, choose (`none)";
    let calls = decode_request(&schema, source).unwrap();
    assert_eq!(field(&calls[0].arguments[0], "limit"), &Value::Integer(3));
    assert!(schema.methods[0].errors.is_empty());
    assert_eq!(schema.methods[1].errors[0].shape.text, "invalid");
    assert_eq!(
        decode_request(&schema, &encode_request(&schema, &calls).unwrap()).unwrap(),
        calls
    );
}

#[test]
fn an_inline_record_has_the_same_union_form_as_a_named_record() {
    let error = diagnostics(
        "PROTOCOL inline VERSION 1,
        TYPE ALIAS Named: { path: Text },
        TYPE ALIAS Conflict [| Named, { count: Integer } |],",
    );
    assert!(error.contains("braces"), "{error}");
}

#[test]
fn records_use_type_aliases_or_named_constructor_slots() {
    assert!(anyx_protocol::Schema::parse("TYPE Quiet Hours { start: Integer }").is_err());
    let schema = anyx_protocol::Schema::parse(
        "PROTOCOL schedule VERSION 1,
         TYPE ALIAS Hours: { start: Integer },
         NAMED TYPE Quiet Hours (schedule: { start: Integer }),
         METHOD set (hours: Quiet Hours) RETURNS NOTHING ERRORS [| |],",
    )
    .unwrap();
    assert!(anyx_protocol::decode_request(&schema, "set (Quiet Hours { start 22 })").is_ok());
    assert!(anyx_protocol::decode_request(&schema, "set { start 22 }").is_err());
}
