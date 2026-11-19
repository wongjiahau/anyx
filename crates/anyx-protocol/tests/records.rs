//! Records whose fields are shapes: a slot anywhere among the words, words
//! alone for a checkbox, and `@` for a field written as its value alone.
mod common;

use anyx_protocol::{Value, decode_request, encode_request};
use common::{diagnostics, schema};

const SEARCH: &str = "PROTOCOL a VERSION 1,
CHOICE Case { smart, any, exact },
CHOICE Order { newest first, by name, largest first },
CHOICE Auth { bearer (token: Text), no auth },
RECORD Search {
    # The pattern to look for.
    pattern: Text,
    (Case DEFAULT smart) case,
    # Searches hidden files too.
    hidden,
    max depth (Integer? DEFAULT null),
    at least (Integer? DEFAULT null) bytes,
    @(order: Order DEFAULT newest first),
    @(auth: Auth? DEFAULT null),
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
        "find { (any) case, @by name, pattern \"ode\", at least 14 bytes, hidden, @bearer \"t\", max depth 3 }",
    );
    assert_eq!(field(&search, "pattern"), &Value::Text("ode".into()));
    assert_eq!(field(&search, "case"), &variant("any", vec![]));
    assert_eq!(field(&search, "hidden"), &variant("true", vec![]));
    assert_eq!(
        field(&search, "max depth"),
        &variant("value _", vec![Value::Integer(3)])
    );
    assert_eq!(
        field(&search, "at least bytes"),
        &variant("value _", vec![Value::Integer(14)])
    );
    assert_eq!(field(&search, "order"), &variant("by name", vec![]));
    assert_eq!(
        field(&search, "auth"),
        &variant(
            "value _",
            vec![variant("bearer _", vec![Value::Text("t".into())])]
        )
    );
}

#[test]
fn fields_left_out_take_their_defaults() {
    let search = find("find { pattern \"ode\" }");
    assert_eq!(field(&search, "case"), &variant("smart", vec![]));
    assert_eq!(field(&search, "hidden"), &variant("false", vec![]));
    assert_eq!(field(&search, "max depth"), &variant("null", vec![]));
    assert_eq!(field(&search, "order"), &variant("newest first", vec![]));
    assert_eq!(field(&search, "auth"), &variant("null", vec![]));
}

#[test]
fn a_record_is_written_back_as_a_reader_writes_it() {
    let schema = schema(SEARCH);
    for (source, written) in [
        (
            "find { pattern \"ode\" }",
            "find { pattern \"ode\", (smart) case }\n",
        ),
        (
            "find { pattern \"ode\", (any) case, hidden, max depth 3, at least 14 bytes, @by name, @no auth }",
            "find { pattern \"ode\", (any) case, hidden, max depth 3, at least 14 bytes, @by name, @no auth }\n",
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
        "find { pattern \"ode\", @exact }",
        "find { pattern \"ode\", any case }",
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
    let error = decode_request(&schema(SEARCH), "find { pattern \"ode\", any case }").unwrap_err();
    assert!(
        format!("{error:?}")
            .contains("a value before a field's words goes in parentheses: `(any) case`"),
        "{error:?}"
    );
}

#[test]
fn two_at_fields_may_not_share_a_form() {
    for (fields, clash) in [
        (
            "@(order: Order), @(sort: Sort)",
            "found `by _` as a value of both `@` fields `order` and `sort`",
        ),
        (
            "@(count: Integer), @(ratio: Float)",
            "found a number as a value of both `@` fields `count` and `ratio`",
        ),
        (
            "@(path: Text), @(at: Instant)",
            "found a string as a value of both `@` fields `path` and `at`",
        ),
        (
            "@(wait: Duration), @(order: Order)",
            "found a duration as a value of both `@` fields `wait` and `order`",
        ),
    ] {
        let source = format!(
            "PROTOCOL a VERSION 1,
            CHOICE Order {{ by (key: Text), (count: Integer) minutes }},
            CHOICE Sort {{ by (rank: Integer) }},
            RECORD Rec {{ {fields} }},"
        );
        let found = diagnostics(&source);
        assert!(found.contains(clash), "{fields}: {found}");
    }
}

#[test]
fn mistakes_in_fields_are_diagnosed() {
    for (fields, message) in [
        (
            "@(auth: Text?)",
            "found the `@` field `auth` of a nullable type without `DEFAULT null`",
        ),
        (
            "between (Integer) and (Integer)",
            "found the field `between _ and _` with 2 slots; expected one",
        ),
        (
            "path: Text DEFAULT \"a\", path: Text",
            "found a second field named `path`",
        ),
    ] {
        let source = format!("PROTOCOL a VERSION 1, RECORD Rec {{ {fields} }},");
        let found = diagnostics(&source);
        assert!(found.contains(message), "{fields}: {found}");
    }
}

#[test]
fn an_at_field_of_a_type_parameter_is_diagnosed() {
    let found = diagnostics("PROTOCOL a VERSION 1, RECORD Box of (item: Type) { @(value: item) },");
    assert!(
        found.contains("found the `@` field `value` of a type parameter"),
        "{found}"
    );
}
