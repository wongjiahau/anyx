//! Effect markers: `READ ONLY` and `IDEMPOTENT` before `METHOD`, and
//! `UNDONE BY` after its results.

mod common;

use anyx_protocol::schema::{Effect, Undo, introspection};
use common::{assert_snapshot, diagnostics, schema};

const DOCUMENTS: &str = "PROTOCOL documents VERSION 1,

READ ONLY METHOD read (path ⁽Text) RETURNS (content ⁽Text) ERRORS [| `missing |],

# Moves the document to the archive. Archiving it again changes nothing.
IDEMPOTENT METHOD archive (path ⁽Text) RETURNS NOTHING ERRORS [| |] UNDONE BY unarchive (path ⁽Text),

IDEMPOTENT METHOD unarchive (path ⁽Text) RETURNS NOTHING ERRORS [| |],

METHOD move (path ⁽Text) to (new path ⁽Text) RETURNS NOTHING
    ERRORS [| `taken |]
    UNDONE BY move (new path ⁽Text) to (path ⁽Text)
    EXAMPLES [move \"a\" to \"b\" RETURNS NOTHING],

METHOD send (path ⁽Text) to (email ⁽Text) RETURNS NOTHING ERRORS [| |],
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
            text: "unarchive (path ⁽Text)".to_string(),
        })
    );
    // The arguments are swapped: `new path` fills the first slot of the
    // undoing `move`.
    assert_eq!(
        schema.methods[3].undone_by,
        Some(Undo {
            method: 3,
            arguments: vec![1, 0],
            text: "move (new path ⁽Text) to (path ⁽Text)".to_string(),
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
fn an_undo_may_name_an_imported_method_declared_after_it() {
    let files = [
        (
            "protocol.protocol.anyx",
            "PROTOCOL documents VERSION 1, USE \"./trash.anyx\",",
        ),
        (
            "trash.anyx",
            "IDEMPOTENT METHOD trash (id ⁽Integer) RETURNS NOTHING ERRORS [| |] UNDONE BY restore (id ⁽Integer),
METHOD restore (id ⁽Integer) RETURNS NOTHING ERRORS [| |],",
        ),
    ];
    let schema = common::parse_files(&files).unwrap();
    let undo = schema.methods[0].undone_by.as_ref().unwrap();
    assert_eq!(schema.methods[undo.method].full_name(), "restore _");
}

#[test]
fn markers_are_formatted_in_place() {
    let formatted = anyx_protocol::format_schema(
        "PROTOCOL t VERSION 1,
READ   ONLY METHOD read (path ⁽Text) RETURNS (content ⁽Text) ERRORS [| |],
IDEMPOTENT METHOD unarchive (path ⁽Text) RETURNS NOTHING ERRORS [| |],
IDEMPOTENT METHOD archive (path ⁽Text) RETURNS NOTHING ERRORS [| `missing, `already archived by someone else |] UNDONE BY unarchive (path ⁽Text) EXAMPLES [archive \"a\" RETURNS NOTHING],
",
    )
    .unwrap();
    assert_eq!(
        formatted,
        "PROTOCOL t VERSION 1,
READ ONLY METHOD read (path ⁽Text) RETURNS (content ⁽Text) ERRORS [| |],
IDEMPOTENT METHOD unarchive (path ⁽Text) RETURNS NOTHING ERRORS [| |],
IDEMPOTENT METHOD archive (path ⁽Text)
    RETURNS NOTHING
    ERRORS [| `missing, `already archived by someone else |]
    UNDONE BY unarchive (path ⁽Text)
    EXAMPLES [archive \"a\" RETURNS NOTHING],
"
    );
}

#[test]
fn the_guide_explains_the_markers() {
    let guide = anyx_protocol::guide(&schema(DOCUMENTS), "");
    assert!(guide.contains("## Effects"));
    assert!(guide.contains("READ ONLY METHOD read (path ⁽Text)"));
    assert!(guide.contains("UNDONE BY move (new path ⁽Text) to (path ⁽Text)"));
}

#[test]
fn mistakes_in_markers_are_diagnosed() {
    let cases = [
        (
            "read only and idempotent together",
            "PROTOCOL a VERSION 1, READ ONLY IDEMPOTENT METHOD read RETURNS NOTHING ERRORS [| |],",
        ),
        (
            "an undo of a read only method",
            "PROTOCOL a VERSION 1, READ ONLY METHOD read (id ⁽Text) RETURNS NOTHING ERRORS [| |] UNDONE BY read (id ⁽Text),",
        ),
        (
            "a slot that is not a parameter",
            "PROTOCOL a VERSION 1, METHOD hide (id ⁽Text) RETURNS NOTHING ERRORS [| |] UNDONE BY show (key ⁽Text), METHOD show (key ⁽Text) RETURNS NOTHING ERRORS [| |],",
        ),
        (
            "a slot in the undo of a method without parameters",
            "PROTOCOL a VERSION 1, METHOD lock RETURNS NOTHING ERRORS [| |] UNDONE BY unlock (id ⁽Text), METHOD unlock (id ⁽Text) RETURNS NOTHING ERRORS [| |],",
        ),
        (
            "a slot of another type than its parameter",
            "PROTOCOL a VERSION 1, METHOD hide (id ⁽Text) RETURNS NOTHING ERRORS [| |] UNDONE BY show (id ⁽Integer), METHOD show (id ⁽Integer) RETURNS NOTHING ERRORS [| |],",
        ),
        (
            "a slot of an unknown type",
            "PROTOCOL a VERSION 1, METHOD hide (id ⁽Text) RETURNS NOTHING ERRORS [| |] UNDONE BY show (id ⁽thing), METHOD show (id ⁽Text) RETURNS NOTHING ERRORS [| |],",
        ),
        (
            "an undo naming no method",
            "PROTOCOL a VERSION 1, METHOD hide (id ⁽Text) RETURNS NOTHING ERRORS [| |] UNDONE BY show (id ⁽Text),",
        ),
        (
            "an undo whose types match no method of its words",
            "PROTOCOL a VERSION 1, NAMED TYPE File Id (value ⁽Text), METHOD hide (id ⁽Text) RETURNS NOTHING ERRORS [| |] UNDONE BY show (id ⁽Text), METHOD show (id ⁽File Id) RETURNS NOTHING ERRORS [| |],",
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
