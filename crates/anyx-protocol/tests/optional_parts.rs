//! Optional parts: a part of a shape in square brackets may be left out of
//! a call, which then holds the part's `DEFAULT`. The full form is the one
//! method or variant; a shorter call is sugar for it.

mod common;

use anyx_protocol::{Value, decode_request};
use common::{diagnostics, schema};

const SOURCE: &str = r#"PROTOCOL p VERSION 1,
TYPE ALIAS Search: { `(hidden: [| `hidden, `null |]) DEFAULT `null },
TYPE ALIAS Order [| `newest [first (count: Integer) DEFAULT first 10], `oldest |],
READ ONLY METHOD find (pattern: Text)
    [in (paths: LIST Text) DEFAULT in ["."]]
    [with (search: Search) DEFAULT with {}]
    RETURNS (paths: LIST Text)
    ERRORS [| |],
READ ONLY METHOD list (order: Order) RETURNS (order: Order) ERRORS [| |],"#;

fn arguments(request: &str) -> Vec<Value> {
    let schema = schema(SOURCE);
    let calls = decode_request(&schema, request).unwrap_or_else(|problem| panic!("{problem}"));
    calls[0].arguments.clone()
}

fn texts(items: &[&str]) -> Value {
    Value::List(
        items
            .iter()
            .map(|item| Value::Text(item.to_string()))
            .collect(),
    )
}

#[test]
fn the_full_form_is_the_one_method() {
    let schema = schema(SOURCE);
    assert_eq!(schema.methods[0].shape.text, "find _ in _ with _");
    assert_eq!(schema.methods[0].shape.parameters.len(), 3);
    let forms: Vec<String> = schema.methods[0]
        .shape
        .forms()
        .into_iter()
        .map(|form| form.text)
        .collect();
    assert_eq!(
        forms,
        [
            "find _ in _ with _",
            "find _ in _",
            "find _ with _",
            "find _"
        ]
    );
}

#[test]
fn a_left_out_part_holds_its_default() {
    let hidden = |on: bool| {
        Value::Record(
            [(
                "hidden".to_string(),
                Value::Variant {
                    shape: if on { "hidden" } else { "null" }.to_string(),
                    arguments: vec![],
                },
            )]
            .into(),
        )
    };
    let x = Value::Text("x".into());
    assert_eq!(
        arguments(r#"find "x""#),
        [x.clone(), texts(&["."]), hidden(false)]
    );
    assert_eq!(
        arguments(r#"find "x" in ["src"]"#),
        [x.clone(), texts(&["src"]), hidden(false)]
    );
    assert_eq!(
        arguments(r#"find "x" with { `hidden }"#),
        [x.clone(), texts(&["."]), hidden(true)]
    );
    assert_eq!(
        arguments(r#"find "x" in ["src"] with { `hidden }"#),
        [x, texts(&["src"]), hidden(true)]
    );
}

#[test]
fn a_variant_may_leave_out_a_part_too() {
    let newest = |count: i64| Value::Variant {
        shape: "newest first _".into(),
        arguments: vec![Value::Integer(count)],
    };
    assert_eq!(arguments("list (`newest)"), [newest(10)]);
    assert_eq!(arguments("list (`newest first 3)"), [newest(3)]);
}

#[test]
fn parts_keep_their_order() {
    let schema = schema(SOURCE);
    let problem = decode_request(&schema, r#"find "x" with {} in ["src"]"#).unwrap_err();
    assert!(
        problem.to_string().contains("find _ with _ in _"),
        "{problem}"
    );
}

#[test]
fn a_part_needs_a_word_and_a_slot() {
    let found = diagnostics(
        "PROTOCOL p VERSION 1,
METHOD scale (shape: Text) [(factor: Integer) DEFAULT 1] RETURNS NOTHING ERRORS [| |],",
    );
    assert!(found.contains("an optional part with no words"), "{found}");
    let found = diagnostics(
        "PROTOCOL p VERSION 1,
METHOD sort (items: LIST Text) [descending DEFAULT descending] RETURNS NOTHING ERRORS [| |],",
    );
    assert!(found.contains("with no slot"), "{found}");
}

#[test]
fn a_default_is_checked_against_its_part() {
    let found = diagnostics(
        r#"PROTOCOL p VERSION 1,
METHOD find (pattern: Text) [in (paths: LIST Text) DEFAULT in "."] RETURNS NOTHING ERRORS [| |],
METHOD look (pattern: Text) [in (paths: LIST Text) DEFAULT at ["."]] RETURNS NOTHING ERRORS [| |],"#,
    );
    assert!(found.contains("in a `DEFAULT`"), "{found}");
    assert!(
        found.contains("expected the part written as a call writes it, `in _`"),
        "{found}"
    );
}

#[test]
fn no_two_methods_may_be_written_alike() {
    let found = diagnostics(
        r#"PROTOCOL p VERSION 1,
METHOD find (pattern: Text) RETURNS NOTHING ERRORS [| |],
METHOD find (pattern: Text) [in (paths: LIST Text) DEFAULT in ["."]] RETURNS NOTHING ERRORS [| |],"#,
    );
    assert!(found.contains("written as `find _`"), "{found}");
    // Told apart by a parameter's type, they are overloads.
    schema(
        r#"PROTOCOL p VERSION 1,
METHOD find (pattern: Integer) RETURNS NOTHING ERRORS [| |],
METHOD find (pattern: Text) [in (paths: LIST Text) DEFAULT in ["."]] RETURNS NOTHING ERRORS [| |],"#,
    );
}

#[test]
fn no_two_variants_of_a_union_may_be_written_alike() {
    let found = diagnostics(
        "PROTOCOL p VERSION 1,
TYPE ALIAS Order [| `newest [first (count: Integer) DEFAULT first 10], `newest |],",
    );
    assert!(found.contains("may both be written as `newest`"), "{found}");
}
