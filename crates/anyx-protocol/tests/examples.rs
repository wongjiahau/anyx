//! `EXAMPLES`: a method's calls and their outcomes, checked against the
//! schema.

mod common;

use anyx_protocol::{Outcome, Value};
use common::{assert_snapshot, diagnostics, parse_files, poentaro, render, schema};

#[test]
fn poentaros_examples_read_with_their_docs_values_and_outcomes() {
    let schema = poentaro();
    let write = schema.method("write _ at revision _ content _").unwrap();
    let docs: Vec<Option<&str>> = write
        .examples
        .iter()
        .map(|example| example.doc.as_deref())
        .collect();
    assert_eq!(
        docs,
        [
            Some("Saving over the revision you read."),
            Some("Someone saved first.")
        ]
    );
    let saving = &write.examples[0];
    assert_eq!(
        saving.call,
        "write \"Hymns/a.poentaro\" at revision 3 content \"title \\\"Ode\\\"\""
    );
    assert_eq!(saving.text, format!("{} RETURNS 4", saving.call));
    assert_eq!(
        saving.arguments,
        [
            Value::Text("Hymns/a.poentaro".to_string()),
            Value::Integer(3),
            Value::Text("title \"Ode\"".to_string()),
        ]
    );
    assert_eq!(saving.outcome, Outcome::Value(Value::Integer(4)));
    let Outcome::Error(Value::Variant { shape, .. }) = &write.examples[1].outcome else {
        panic!("expected an error, found {:?}", write.examples[1].outcome);
    };
    assert_eq!(shape, "conflict at revision _ content _");
    assert!(schema.method("read _").unwrap().examples.is_empty());
}

#[test]
fn an_example_may_go_without_a_doc_and_return_nothing() {
    let schema = schema(
        "PROTOCOL t VERSION 1,
METHOD add (a: Integer) to (b: Integer) RETURNS (sum: Integer) ERRORS [] EXAMPLES [add 1 to 2 RETURNS 3],
METHOD ping RETURNS NOTHING ERRORS [] EXAMPLES [
    # Always answers.
    ping RETURNS NOTHING,
],
",
    );
    let add = &schema.methods[0].examples[0];
    assert_eq!(add.doc, None);
    assert_eq!(add.text, "add 1 to 2 RETURNS 3");
    let ping = &schema.methods[1].examples[0];
    assert_eq!(ping.doc.as_deref(), Some("Always answers."));
    assert_eq!(ping.call, "ping");
}

#[test]
fn examples_are_formatted_one_per_line_after_a_break() {
    let formatted = anyx_protocol::format_schema(
        "PROTOCOL t VERSION 1,
METHOD add (first: Integer) to (second: Integer) RETURNS (total: Integer) ERRORS [`overflow] EXAMPLES [add 1 to 2 RETURNS 3, # Too big.
add 9223372036854775807 to 1 ERROR `overflow],
METHOD ping RETURNS NOTHING ERRORS [] EXAMPLES [ping RETURNS NOTHING],
",
    )
    .unwrap();
    // Like `ERRORS`, `EXAMPLES` starts a line when its method doesn't fit
    // on one.
    assert_eq!(
        formatted,
        "PROTOCOL t VERSION 1,
METHOD add (first: Integer) to (second: Integer)
    RETURNS (total: Integer)
    ERRORS [`overflow]
    EXAMPLES [
        add 1 to 2 RETURNS 3,
        # Too big.
        add 9223372036854775807 to 1 ERROR `overflow,
    ],
METHOD ping RETURNS NOTHING ERRORS [] EXAMPLES [ping RETURNS NOTHING],
"
    );
}

#[test]
fn an_imported_method_example_uses_its_flat_name() {
    let files = [
        (
            "protocol.protocol.anyx",
            "PROTOCOL notes VERSION 1, USE \"./files.protocol.anyx\",",
        ),
        (
            "files.protocol.anyx",
            "METHOD read (path: Text) RETURNS (content: Text) ERRORS [] EXAMPLES [
    # Reading a file.
    read \"a\" RETURNS \"x\",
],",
        ),
    ];
    let schema = parse_files(&files)
        .unwrap_or_else(|diagnostics| panic!("{}", render(&files, &diagnostics)));
    let example = &schema.methods[0].examples[0];
    assert_eq!(example.doc.as_deref(), Some("Reading a file."));
    assert_eq!(example.call, "read \"a\"");
    let files = [
        (
            "protocol.protocol.anyx",
            "PROTOCOL notes VERSION 1, USE \"./files.protocol.anyx\",",
        ),
        (
            "files.protocol.anyx",
            "METHOD read (path: Text) RETURNS (content: Text) ERRORS [] EXAMPLES [files/read \"a\" RETURNS \"x\"],",
        ),
    ];
    let diagnostics = parse_files(&files).unwrap_err();
    assert_snapshot(
        "tests/snapshots/scoped_example_diagnostics.txt",
        &render(&files, &diagnostics),
    );
}

#[test]
fn mistakes_in_examples_are_diagnosed() {
    let cases = [
        // A call of another method.
        "PROTOCOL t VERSION 1,
METHOD ping RETURNS NOTHING ERRORS [],
METHOD add (a: Integer) to (b: Integer) RETURNS (sum: Integer) ERRORS [] EXAMPLES [ping RETURNS NOTHING],
",
        // An argument of the wrong type.
        "PROTOCOL t VERSION 1,
METHOD add (a: Integer) to (b: Integer) RETURNS (sum: Integer) ERRORS [] EXAMPLES [add \"1\" to 2 RETURNS 3],
",
        // A value it doesn't return.
        "PROTOCOL t VERSION 1,
METHOD add (a: Integer) to (b: Integer) RETURNS (sum: Integer) ERRORS [] EXAMPLES [add 1 to 2 RETURNS \"3\"],
",
        // A value from a method that returns nothing.
        "PROTOCOL t VERSION 1,
METHOD ping RETURNS NOTHING ERRORS [] EXAMPLES [ping RETURNS 3],
",
        // `NOTHING` from a method that returns a value.
        "PROTOCOL t VERSION 1,
METHOD add (a: Integer) to (b: Integer) RETURNS (sum: Integer) ERRORS [] EXAMPLES [add 1 to 2 RETURNS NOTHING],
",
        // An error from a method without errors.
        "PROTOCOL t VERSION 1,
METHOD add (a: Integer) to (b: Integer) RETURNS (sum: Integer) ERRORS [] EXAMPLES [add 1 to 2 ERROR `overflow],
",
        // An error that isn't one of the method's.
        "PROTOCOL t VERSION 1,
METHOD add (a: Integer) to (b: Integer) RETURNS (sum: Integer) ERRORS [`overflow] EXAMPLES [add 1 to 2 ERROR `missing],
",
        // An empty list.
        "PROTOCOL t VERSION 1,
METHOD ping RETURNS NOTHING ERRORS [] EXAMPLES [],
",
        // Examples aren't checked while the schema has other mistakes.
        "PROTOCOL t VERSION 1,
METHOD add (a: unknown) RETURNS NOTHING ERRORS [] EXAMPLES [add 1 RETURNS 3],
",
    ];
    let rendered: Vec<String> = cases.iter().map(|source| diagnostics(source)).collect();
    assert_snapshot(
        "tests/snapshots/example_diagnostics.txt",
        &rendered.join("\n\n"),
    );
}

#[test]
fn an_example_reads_its_values_with_their_defaults() {
    let schema = schema(
        "PROTOCOL t VERSION 1,
RECORD Reminder { at: Instant, times (Integer) DEFAULT times 3 },
METHOD next RETURNS (reminder: Reminder) ERRORS [] EXAMPLES [next RETURNS { at \"2026-11-14T06:00:00Z\" }],
",
    );
    let Outcome::Value(Value::Record(fields)) = &schema.methods[0].examples[0].outcome else {
        panic!("expected a record");
    };
    assert_eq!(fields["times"], Value::Integer(3));
    assert!(matches!(fields["at"], Value::Instant(_)));
}
