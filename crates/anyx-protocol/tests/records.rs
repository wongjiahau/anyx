//! Records whose fields are shapes: a slot anywhere among the words, words
//! alone for a checkbox, and `@` for a field written as its value alone.
mod common;

use anyx_protocol::{Value, decode_request, encode_request};
use common::{diagnostics, schema};

const SEARCH: &str = "PROTOCOL a VERSION 1,
CHOICE Case { `smart, `any, `exact },
CHOICE Order { `newest first, `by name, `largest first },
CHOICE Auth { `bearer (token: Text), `no auth },
RECORD Search {
    # The pattern to look for.
    pattern: Text,
    (Case) case DEFAULT (`smart) case,
    # Searches hidden files too.
    hidden,
    max depth (Integer?) DEFAULT max depth (`null),
    at least (Integer?) bytes DEFAULT at least (`null) bytes,
    @(order: Order) DEFAULT @(`newest first),
    @(auth: Auth) DEFAULT @(`no auth),
},
METHOD find (search: Search) RETURNS (count: Integer),";

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
        "find { (`any) case, @(`by name), pattern \"ode\", at least 14 bytes, hidden, @(`bearer \"t\"), max depth 3 }",
    );
    assert_eq!(field(&search, "pattern"), &Value::Text("ode".into()));
    assert_eq!(field(&search, "case"), &variant("any", vec![]));
    assert_eq!(field(&search, "hidden"), &variant("true", vec![]));
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
    assert_eq!(field(&search, "hidden"), &variant("false", vec![]));
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
            "find { pattern \"ode\", (`any) case, hidden, max depth 3, at least 14 bytes, @(`by name), @(`bearer \"t\") }",
            "find {
    pattern \"ode\",
    (`any) case,
    hidden,
    max depth 3,
    at least 14 bytes,
    @(`by name),
    @(`bearer \"t\"),
}
",
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
        "find { pattern \"ode\", @(`exact) }",
        "find { pattern \"ode\", `any case }",
        "find { pattern \"ode\", @\"x\" }",
    ] {
        let error = decode_request(&schema, source).unwrap_err();
        assert!(
            format!("{error:?}").contains("expected a field: `pattern …`, `… case`, `hidden`"),
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
fn a_duration_finds_its_at_field_by_its_units() {
    let schema = schema(
        "PROTOCOL a VERSION 1,
        CHOICE Order { `newest first, `by name },
        RECORD Wait { @(wait: Duration) DEFAULT @1 seconds, @(order: Order) DEFAULT @(`newest first) },
        METHOD find (wait: Wait) RETURNS (count: Integer),",
    );
    for (source, milliseconds) in [
        ("find { @5 minutes, @(`by name) }", 300_000),
        ("find { @(`by name), @1 hours 30 minutes }", 5_400_000),
        ("find { @1 minute }", 60_000),
    ] {
        let calls = decode_request(&schema, source).unwrap_or_else(|error| panic!("{error:?}"));
        assert_eq!(
            field(&calls[0].arguments[0], "wait"),
            &Value::Duration(std::time::Duration::from_millis(milliseconds)),
            "{source}"
        );
    }
}

#[test]
fn two_at_fields_may_not_share_a_form() {
    for (fields, clash) in [
        (
            "@(order: Order), @(sort: Sort)",
            "found `` `by _ `` as a value of both `@` fields `order` and `sort`",
        ),
        (
            "@(count: Integer), @(ratio: Float)",
            "found a number as a value of both `@` fields `count` and `ratio`",
        ),
        (
            "@(path: Text), @(at: Instant)",
            "found a string as a value of both `@` fields `path` and `at`",
        ),
    ] {
        let source = format!(
            "PROTOCOL a VERSION 1,
            CHOICE Order {{ `by (key: Text), `(count: Integer) minutes }},
            CHOICE Sort {{ `by (rank: Integer) }},
            RECORD Rec {{ {fields} }},"
        );
        let found = diagnostics(&source);
        assert!(found.contains(clash), "{fields}: {found}");
    }
    // The mark tells the variant `` `5 minutes `` from the duration
    // `5 minutes`, so the two fields don't clash.
    assert!(
        anyx_protocol::Schema::parse(
            "PROTOCOL a VERSION 1,
            CHOICE Order { `(count: Integer) minutes },
            RECORD Rec { @(wait: Duration), @(order: Order) },"
        )
        .is_ok()
    );
}

#[test]
fn mistakes_in_fields_are_diagnosed() {
    for (fields, message) in [
        (
            "between (Integer) and (Integer) DEFAULT between 1",
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
            "found a `DEFAULT` on the checkbox `hidden`; expected none",
        ),
        (
            "age (Integer) DEFAULT age 0, age (Integer)",
            "found a second field named `age`",
        ),
    ] {
        let source = format!("PROTOCOL a VERSION 1, RECORD Rec {{ {fields} }},");
        let found = diagnostics(&source);
        assert!(found.contains(message), "{fields}: {found}");
    }
}

#[test]
fn an_at_field_of_a_type_parameter_is_diagnosed() {
    let found = diagnostics("PROTOCOL a VERSION 1, RECORD Box of (Item: TYPE) { @(value: Item) },");
    assert!(
        found.contains("found the `@` field `value` of a type parameter"),
        "{found}"
    );
}

const RANGE: &str = "PROTOCOL a VERSION 1,
TYPE ALIAS Depth: Integer . WHERE [0 <= _],
RECORD Search {
    depth from (Depth) to (Depth?),
    between (Integer) and (Integer) bytes DEFAULT between 1 and 9 bytes,
},
METHOD find (search: Search) RETURNS (count: Integer),";

#[test]
fn a_field_with_several_slots_holds_a_value_for_each() {
    let schema = schema(RANGE);
    let calls = decode_request(&schema, "find { depth from 1 to 3 }").unwrap();
    let search = &calls[0].arguments[0];
    assert_eq!(
        field(search, "depth from to"),
        &Value::Tuple(vec![Value::Integer(1), Value::Integer(3)])
    );
    assert_eq!(
        field(search, "between and bytes"),
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
        format!("{error:?}")
            .contains("in `depth from to`: value `-1` is not a `(Integer) WHERE [0 <= _]`"),
        "{error:?}"
    );
}

#[cfg(feature = "json")]
#[test]
fn a_field_with_several_slots_is_an_array_in_json() {
    use anyx_protocol::json::{decode_request_json, encode_request_json};
    let schema = schema(RANGE);
    let json = decode_request_json(&schema, "find { depth from 1 to (`null) }").unwrap();
    assert!(
        json.contains(r#""depthFromTo":[1,{"kind":"null"}]"#),
        "{json}"
    );
    let arguments = r#"[{"depthFromTo": [1, {"kind": "null"}], "betweenAndBytes": [1, 9]}]"#;
    assert_eq!(
        encode_request_json(&schema, "find _", arguments).unwrap(),
        "find { depth from 1 to (`null), between 1 and 9 bytes }\n"
    );
}
