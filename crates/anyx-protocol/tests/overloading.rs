//! Methods of one shape with different parameter types, each call resolved
//! by its arguments' types, and `value AS: type`, which states the type of
//! a value that has none of its own.

mod common;

use anyx_protocol::schema::Undo;
use anyx_protocol::{Call, Value, decode_request, encode_request};
use common::schema;

const TAGS: &str = "PROTOCOL a VERSION 1,
TYPE file id: TEXT,
CHOICE shade { dark, light },
CHOICE colour { dark, red },
CHOICE mood { calm, wild },
RECORD point { x: INTEGER, y: INTEGER },
RECORD size { x: INTEGER, label: OPTIONAL: TEXT },
RECORD width { x: INTEGER },
METHOD tag (names: LIST: TEXT) RETURNS NOTHING,
METHOD tag (ids: LIST: INTEGER) RETURNS NOTHING,
METHOD open (path: TEXT) RETURNS NOTHING,
METHOD open (id: file id) RETURNS NOTHING,
METHOD scale (factor: FLOAT) RETURNS NOTHING,
METHOD scale (times: INTEGER) RETURNS NOTHING,
METHOD paint (shade: shade) RETURNS NOTHING,
METHOD paint (colour: colour) RETURNS NOTHING,
METHOD feel (mood: mood) RETURNS NOTHING,
METHOD move to (point: point) RETURNS NOTHING,
METHOD fit (size: size) RETURNS NOTHING,
METHOD show (shown: BOOLEAN) RETURNS NOTHING,
METHOD wait (length: DURATION) RETURNS NOTHING,
METHOD read (id: file id) RETURNS NOTHING,";

fn text(value: &str) -> Value {
    Value::Text(value.to_string())
}

fn list(items: Vec<Value>) -> Value {
    Value::List(items)
}

/// Which method each call of `request` is of, by its index, and the
/// arguments.
fn calls(request: &str) -> Vec<(usize, Vec<Value>)> {
    decode_request(&schema(TAGS), request)
        .unwrap_or_else(|problem| panic!("{request}: {problem}"))
        .into_iter()
        .map(|call| (call.method, call.arguments))
        .collect()
}

fn message(request: &str) -> String {
    decode_request(&schema(TAGS), request)
        .map(|calls| panic!("{request}: expected a problem, read {calls:?}"))
        .unwrap_err()
        .message
}

#[test]
fn a_call_resolves_to_the_overload_its_arguments_fit() {
    assert_eq!(
        calls("tag [\"hymn\", \"ode\"]"),
        [(0, vec![list(vec![text("hymn"), text("ode")])])]
    );
    assert_eq!(
        calls("tag [1, 2]"),
        [(1, vec![list(vec![Value::Integer(1), Value::Integer(2)])])]
    );
    assert_eq!(calls("tag ([] AS: LIST: TEXT)"), [(0, vec![list(vec![])])]);
    assert_eq!(
        calls("tag ([] AS: LIST: INTEGER)"),
        [(1, vec![list(vec![])])]
    );
    assert_eq!(calls("open \"a\""), [(2, vec![text("a")])]);
    assert_eq!(calls("open (\"a\" AS: file id)"), [(3, vec![text("a")])]);
    assert_eq!(
        calls("open (\"a\" AS file id)"),
        [(3, vec![text("a")])],
        "`AS: x` is `AS (x)`"
    );
    assert_eq!(calls("scale 3.0"), [(4, vec![Value::Float(3.0)])]);
    assert_eq!(calls("scale 3"), [(5, vec![Value::Integer(3)])]);
    assert_eq!(calls("paint (dark AS: shade)")[0].0, 6);
    assert_eq!(calls("paint (dark AS: colour)")[0].0, 7);
    assert_eq!(calls("paint (red)")[0].0, 7, "`red` is only a `colour`");
}

#[test]
fn a_value_with_no_type_of_its_own_needs_an_annotation() {
    assert_eq!(
        message("tag []"),
        "in `tag _`: `[]` has no type of its own, so no overload of `tag _` can be chosen by it; annotate it with the type of the one to call, as in `tag ([] AS: LIST: TEXT)`; the overloads are `tag (names: LIST: TEXT)`, `tag (ids: LIST: INTEGER)`"
    );
    assert_eq!(
        message("paint (dark)"),
        "in `paint _`: `dark` has no type of its own, so no overload of `paint _` can be chosen by it; annotate it with the type of the one to call, as in `paint (dark AS: shade)`; the overloads are `paint (shade: shade)`, `paint (colour: colour)`"
    );
    // Even when the method has no overloads, so that adding one never
    // breaks a call.
    assert_eq!(
        message("read \"a\""),
        "in `id` of `read _`: `\"a\"` has the type TEXT of its own, not `file id`; annotate it, as in `read (\"a\" AS: file id)`"
    );
    assert_eq!(
        message("feel (calm), read \"a\""),
        "in `id` of `read _`: `\"a\"` has the type TEXT of its own, not `file id`; annotate it, as in `read (\"a\" AS: file id)`",
        "the whole request is invalid, so none of its calls run"
    );
}

#[test]
fn a_record_a_variant_a_boolean_and_a_duration_have_types_of_their_own() {
    assert_eq!(calls("feel (calm)")[0].0, 8, "only `mood` has `calm`");
    assert_eq!(
        calls("move to { x 1, y 2 }")[0].0,
        9,
        "only `point` has `y`"
    );
    assert_eq!(calls("fit { x 1, label \"a\" }")[0].0, 10);
    assert_eq!(
        message("fit { x 1 }"),
        "in `size` of `fit _`: `{ x 1 }` has no type of its own; annotate it, as in `fit ({ x 1 } AS: size)`",
        "`size` and `width` both fit; `point` needs a `y`"
    );
    assert_eq!(calls("show (true)"), [(11, vec![Value::Boolean(true)])]);
    assert_eq!(calls("wait (1 hour)")[0].0, 12);
}

#[test]
fn a_call_no_overload_fits_lists_them() {
    assert_eq!(
        message("tag [1.5]"),
        "found `tag _` with an argument of the type `LIST: FLOAT`; expected the types of one of its overloads: `tag (names: LIST: TEXT)`, `tag (ids: LIST: INTEGER)`; a value of a named type, or of `BYTES` or `INSTANT`, is written like another type's, so annotate it, as in `(\"abc\" AS: file id)`"
    );
}

#[test]
fn an_annotation_is_a_checked_claim() {
    assert_eq!(
        message("tag ([1] AS: LIST: TEXT)"),
        "in `names` of `tag _`: found the number `1`; expected a string, as in `\"text\"`"
    );
    assert_eq!(
        message("feel (calm AS: shade)"),
        "in `mood` of `feel _`: found `AS: shade`; expected `AS: mood`, the type of `mood`"
    );
    assert_eq!(
        message("feel (calm AS: weather)"),
        "found `weather` after `AS`, which names no type of the schema's root; expected a type written as a parameter's is: `TEXT`, `LIST: TEXT`, a type of the scope, such as `file id`, or the full path of a `PUBLIC` one in another, such as `common/file id`"
    );
    assert_eq!(
        message("tag [(\"a\" AS: TEXT)]"),
        "found an annotation `AS` inside a value; expected one only around a whole argument of a call, as in `tag ([] AS: LIST: TEXT)`; a value inside another is read by the type around it"
    );
}

#[test]
fn a_request_states_the_types_it_depends_on() {
    let schema = schema(TAGS);
    let calls = vec![
        Call {
            method: 0,
            arguments: vec![list(vec![])],
        },
        Call {
            method: 1,
            arguments: vec![list(vec![Value::Integer(1)])],
        },
        Call {
            method: 3,
            arguments: vec![text("a")],
        },
        Call {
            method: 4,
            arguments: vec![Value::Float(3.0)],
        },
        Call {
            method: 6,
            arguments: vec![Value::Variant {
                shape: "dark".to_string(),
                arguments: vec![],
            }],
        },
        Call {
            method: 8,
            arguments: vec![Value::Variant {
                shape: "calm".to_string(),
                arguments: vec![],
            }],
        },
    ];
    let request = encode_request(&schema, &calls).unwrap();
    assert_eq!(
        request,
        "tag ([] AS: LIST: TEXT),
tag [1],
open (\"a\" AS file id),
scale 3.0,
paint (dark AS shade),
feel (calm),
"
    );
    assert_eq!(decode_request(&schema, &request).unwrap(), calls);
}

#[test]
fn undone_by_names_one_overload() {
    let schema = schema(
        "PROTOCOL a VERSION 1,
TYPE file id: TEXT,
METHOD hide (path: TEXT) RETURNS NOTHING UNDONE BY show (path: TEXT),
METHOD hide (id: file id) RETURNS NOTHING UNDONE BY show (id: file id),
METHOD show (path: TEXT) RETURNS NOTHING,
METHOD show (id: file id) RETURNS NOTHING,",
    );
    assert_eq!(
        schema.methods[0].undone_by.as_ref().map(|undo| undo.method),
        Some(2)
    );
    assert_eq!(
        schema.methods[1].undone_by,
        Some(Undo {
            method: 3,
            arguments: vec![0],
            text: "show (id: file id)".to_string(),
        })
    );
}

#[test]
fn an_example_annotates_as_a_request_does() {
    let parsed = anyx_protocol::Schema::parse(
        "PROTOCOL a VERSION 1, TYPE file id: TEXT,
METHOD read (id: file id) RETURNS NOTHING EXAMPLES [read (\"a\" AS: file id) RETURNS NOTHING],",
    );
    assert!(parsed.is_ok(), "{parsed:?}");
    let unannotated = common::diagnostics(
        "PROTOCOL a VERSION 1, TYPE file id: TEXT,
METHOD read (id: file id) RETURNS NOTHING EXAMPLES [read \"a\" RETURNS NOTHING],",
    );
    assert!(
        unannotated.contains(
            "in an example of `read _`: in `id` of `read _`: `\"a\"` has the type TEXT of its own, not `file id`; annotate it, as in `read (\"a\" AS: file id)`"
        ),
        "{unannotated}"
    );
}

/// JSON can't tell `3` from `3.0`, nor a `file id` from a `TEXT`, so it
/// names an overload by its typed name, `scale (FLOAT)`.
#[cfg(feature = "json")]
#[test]
fn json_names_an_overload_with_its_parameters_types() {
    use anyx_protocol::json::{decode_request_json, encode_request_json};
    let schema = schema(TAGS);
    assert_eq!(
        encode_request_json(&schema, "scale (FLOAT)", "[3]").unwrap(),
        "scale 3.0\n"
    );
    assert_eq!(
        encode_request_json(&schema, "scale (INTEGER)", "[3]").unwrap(),
        "scale 3\n"
    );
    assert_eq!(
        encode_request_json(&schema, "open (file id)", "[\"a\"]").unwrap(),
        "open (\"a\" AS file id)\n"
    );
    assert_eq!(
        encode_request_json(&schema, "scale _", "[3]").unwrap_err(),
        "found the method `scale _`, which has overloads; expected one of them by its name with its parameters' types: `scale (FLOAT)`, `scale (INTEGER)`"
    );
    // A method with no overloads goes by its full name, or its typed one.
    assert_eq!(
        encode_request_json(&schema, "read _", "[\"a\"]").unwrap(),
        "read (\"a\" AS file id)\n"
    );
    assert_eq!(
        encode_request_json(&schema, "read (file id)", "[\"a\"]").unwrap(),
        "read (\"a\" AS file id)\n"
    );
    assert_eq!(
        decode_request_json(
            &schema,
            "tag ([] AS: LIST: INTEGER), read (\"a\" AS: file id)"
        )
        .unwrap(),
        r#"[{"arguments":[[]],"method":"tag (LIST: INTEGER)"},{"arguments":["a"],"method":"read _"}]"#
    );
}

#[test]
fn the_guide_lists_each_shape_s_overloads_together() {
    let guide = anyx_protocol::guide(&schema(TAGS), "");
    assert!(
        guide.contains(
            "argument whose value doesn't show its type (see Values):

- `tag (names: LIST: TEXT)`, `tag (ids: LIST: INTEGER)`
- `open (path: TEXT)`, `open (id: file id)`
- `scale (factor: FLOAT)`, `scale (times: INTEGER)`
- `paint (shade: shade)`, `paint (colour: colour)`
"
        ),
        "{guide}"
    );
    assert!(guide.contains("`tag ([] AS: LIST: TEXT)`"), "{guide}");
}
