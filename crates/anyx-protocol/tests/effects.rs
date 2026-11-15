//! Effect markers: `READ ONLY` and `IDEMPOTENT` before `METHOD`, and
//! `UNDONE BY` after its results.

mod common;

use anyx_protocol::schema::{Effect, Undo, introspection};
use common::{assert_snapshot, diagnostics, schema};

const DOCUMENTS: &str = "PROTOCOL documents VERSION 1,

READ ONLY METHOD read (path: TEXT) RETURNS (content: TEXT) ERRORS [missing],

# Moves the document to the archive. Archiving it again changes nothing.
IDEMPOTENT METHOD archive (path: TEXT) RETURNS NOTHING UNDONE BY unarchive (path: TEXT),

IDEMPOTENT METHOD unarchive (path: TEXT) RETURNS NOTHING,

METHOD move (path: TEXT) to (new path: TEXT) RETURNS NOTHING
    ERRORS [taken]
    UNDONE BY move (new path: TEXT) to (path: TEXT)
    EXAMPLES [move \"a\" to \"b\" RETURNS NOTHING],

METHOD send (path: TEXT) to (email: TEXT) RETURNS NOTHING,
";

#[test]
fn markers_are_read_into_each_method() {
    let schema = schema(DOCUMENTS);
    let effects: Vec<(&str, Effect)> = schema
        .methods
        .iter()
        .map(|method| (method.shape.text.as_str(), method.effect))
        .collect();
    assert_eq!(
        effects,
        [
            ("read _", Effect::ReadOnly),
            ("archive _", Effect::Idempotent),
            ("unarchive _", Effect::Idempotent),
            ("move _ to _", Effect::Unmarked),
            ("send _ to _", Effect::Unmarked),
        ]
    );
    assert!(Effect::ReadOnly.idempotent());
    assert!(!Effect::Unmarked.idempotent());
    assert_eq!(
        schema.methods[0].doc, None,
        "`READ ONLY` is not part of the doc"
    );
    assert_eq!(
        schema.methods[1].doc.as_deref(),
        Some("Moves the document to the archive. Archiving it again changes nothing.")
    );
    assert_eq!(
        schema.methods[1].undone_by,
        Some(Undo {
            method: 2,
            arguments: vec![0],
            text: "unarchive (path: TEXT)".to_string(),
        })
    );
    // The arguments are swapped: `new path` fills the first slot of the
    // undoing `move`.
    assert_eq!(
        schema.methods[3].undone_by,
        Some(Undo {
            method: 3,
            arguments: vec![1, 0],
            text: "move (new path: TEXT) to (path: TEXT)".to_string(),
        })
    );
    assert_eq!(schema.methods[4].undone_by, None);
    assert_eq!(schema.methods[3].examples.len(), 1);
}

#[test]
fn the_built_in_schema_call_is_read_only() {
    assert_eq!(introspection().effect, Effect::ReadOnly);
}

#[test]
fn an_undo_may_name_a_method_declared_after_it_in_its_scope() {
    let files = [
        ("protocol.anyx", "PROTOCOL documents VERSION 1,"),
        (
            "trash.anyx",
            "IDEMPOTENT METHOD trash (id: INTEGER) RETURNS NOTHING UNDONE BY restore (id: INTEGER),
METHOD restore (id: INTEGER) RETURNS NOTHING,",
        ),
    ];
    let schema = common::parse_files(&files).unwrap();
    let undo = schema.methods[0].undone_by.as_ref().unwrap();
    assert_eq!(schema.methods[undo.method].full_name(), "trash/restore _");
}

#[test]
fn markers_are_formatted_in_place() {
    let formatted = anyx_protocol::format_schema(
        "PROTOCOL t VERSION 1,
READ   ONLY METHOD read (path: TEXT) RETURNS (content: TEXT),
IDEMPOTENT METHOD unarchive (path: TEXT) RETURNS NOTHING,
IDEMPOTENT METHOD archive (path: TEXT) RETURNS NOTHING ERRORS [missing, already archived by someone else] UNDONE BY unarchive (path: TEXT) EXAMPLES [archive \"a\" RETURNS NOTHING],
",
    )
    .unwrap();
    assert_eq!(
        formatted,
        "PROTOCOL t VERSION 1,
READ ONLY METHOD read (path: TEXT) RETURNS content: TEXT,
IDEMPOTENT METHOD unarchive (path: TEXT) RETURNS NOTHING,
IDEMPOTENT METHOD archive (path: TEXT)
    RETURNS NOTHING
    ERRORS [missing, already archived by someone else]
    UNDONE BY unarchive (path: TEXT)
    EXAMPLES [archive \"a\" RETURNS NOTHING],
"
    );
}

#[test]
fn the_guide_explains_the_markers() {
    let guide = anyx_protocol::guide(&schema(DOCUMENTS), "");
    assert!(guide.contains("## Effects"));
    assert!(guide.contains("READ ONLY METHOD read (path: TEXT)"));
    assert!(guide.contains("UNDONE BY move (new path: TEXT) to (path: TEXT)"));
}

#[test]
fn mistakes_in_markers_are_diagnosed() {
    let cases = [
        (
            "read only and idempotent together",
            "PROTOCOL a VERSION 1, READ ONLY IDEMPOTENT METHOD read RETURNS NOTHING,",
        ),
        (
            "an undo of a read only method",
            "PROTOCOL a VERSION 1, READ ONLY METHOD read (id: TEXT) RETURNS NOTHING UNDONE BY read (id: TEXT),",
        ),
        (
            "a slot that is not a parameter",
            "PROTOCOL a VERSION 1, METHOD hide (id: TEXT) RETURNS NOTHING UNDONE BY show (key: TEXT), METHOD show (key: TEXT) RETURNS NOTHING,",
        ),
        (
            "a slot in the undo of a method without parameters",
            "PROTOCOL a VERSION 1, METHOD lock RETURNS NOTHING UNDONE BY unlock (id: TEXT), METHOD unlock (id: TEXT) RETURNS NOTHING,",
        ),
        (
            "a slot of another type than its parameter",
            "PROTOCOL a VERSION 1, METHOD hide (id: TEXT) RETURNS NOTHING UNDONE BY show (id: INTEGER), METHOD show (id: INTEGER) RETURNS NOTHING,",
        ),
        (
            "a slot of an unknown type",
            "PROTOCOL a VERSION 1, METHOD hide (id: TEXT) RETURNS NOTHING UNDONE BY show (id: thing), METHOD show (id: TEXT) RETURNS NOTHING,",
        ),
        (
            "an undo naming no method",
            "PROTOCOL a VERSION 1, METHOD hide (id: TEXT) RETURNS NOTHING UNDONE BY show (id: TEXT),",
        ),
        (
            "an undo whose types match no method of its words",
            "PROTOCOL a VERSION 1, TYPE file id: TEXT, METHOD hide (id: TEXT) RETURNS NOTHING UNDONE BY show (id: TEXT), METHOD show (id: file id) RETURNS NOTHING,",
        ),
    ];
    let rendered: Vec<String> = cases
        .iter()
        .map(|(title, source)| format!("== {title}\n{source}\n{}\n", diagnostics(source)))
        .collect();
    assert_snapshot(
        "tests/snapshots/effect_diagnostics.txt",
        &rendered.join("\n"),
    );
}
