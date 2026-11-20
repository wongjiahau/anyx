//! Methods of one shape with different parameter types, each call resolved
//! by its arguments' types, and `value AS: type`, which states the type of
//! a value that has none of its own.

mod common;

use anyx_protocol::schema::Undo;
use anyx_protocol::{Call, Value, decode_request, encode_request};
use common::schema;

const TAGS: &str = "PROTOCOL a VERSION 1,
NAMED TYPE File Id (Text),
TYPE ALIAS Shade: UNION [`dark, `light],
TYPE ALIAS Colour: UNION [`dark, `red],
TYPE ALIAS Mood: UNION [`calm, `wild],
RECORD Point { x: Integer, y: Integer },
RECORD Size { x: Integer, label (Text) DEFAULT label \"\" },
RECORD Width { x: Integer },
METHOD tag (names: LIST Text) RETURNS NOTHING ERRORS [],
METHOD tag (ids: LIST Integer) RETURNS NOTHING ERRORS [],
METHOD open (path: Text) RETURNS NOTHING ERRORS [],
METHOD open (id: File Id) RETURNS NOTHING ERRORS [],
METHOD scale (factor: Float) RETURNS NOTHING ERRORS [],
METHOD scale (times: Integer) RETURNS NOTHING ERRORS [],
METHOD paint (shade: Shade) RETURNS NOTHING ERRORS [],
METHOD paint (colour: Colour) RETURNS NOTHING ERRORS [],
METHOD feel (mood: Mood) RETURNS NOTHING ERRORS [],
METHOD move to (point: Point) RETURNS NOTHING ERRORS [],
METHOD fit (size: Size) RETURNS NOTHING ERRORS [],
METHOD show (shown: Boolean) RETURNS NOTHING ERRORS [],
METHOD wait (length: Duration) RETURNS NOTHING ERRORS [],
METHOD read (id: File Id) RETURNS NOTHING ERRORS [],";

fn text(value: &str) -> Value {
    Value::Text(value.to_string())
}

fn file_id(id: &str) -> Value {
    Value::Variant {
        shape: "File Id _".to_string(),
        arguments: vec![text(id)],
    }
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
    assert_eq!(calls("tag ([] AS: LIST Text)"), [(0, vec![list(vec![])])]);
    assert_eq!(
        calls("tag ([] AS: LIST Integer)"),
        [(1, vec![list(vec![])])]
    );
    assert_eq!(calls("open \"a\""), [(2, vec![text("a")])]);
    assert_eq!(calls("open (File Id \"a\")"), [(3, vec![file_id("a")])]);
    assert_eq!(
        calls("tag ([] AS (LIST Text))"),
        [(0, vec![list(vec![])])],
        "`AS: x` is `AS (x)`"
    );
    assert_eq!(calls("scale 3.0"), [(4, vec![Value::Float(3.0)])]);
    assert_eq!(calls("scale 3"), [(5, vec![Value::Integer(3)])]);
    assert_eq!(calls("paint (`dark AS: Shade)")[0].0, 6);
    assert_eq!(calls("paint (`dark AS: Colour)")[0].0, 7);
    assert_eq!(calls("paint (`red)")[0].0, 7, "`red` is only a `colour`");
}

#[test]
fn a_value_with_no_type_of_its_own_needs_an_annotation() {
    assert_eq!(
        message("tag []"),
        "in `tag _`: `[]` has no type of its own, so no overload of `tag _` can be chosen by it; annotate it with the type of the one to call, as in `tag ([] AS: LIST Text)`; the overloads are `tag (names: LIST Text)`, `tag (ids: LIST Integer)`"
    );
    assert_eq!(
        message("paint (`dark)"),
        "in `paint _`: `` `dark `` has no type of its own, so no overload of `paint _` can be chosen by it; annotate it with the type of the one to call, as in `` paint (`dark AS: Shade) ``; the overloads are `paint (shade: Shade)`, `paint (colour: Colour)`"
    );
    // Even when the method has no overloads, so that adding one never
    // breaks a call.
    assert_eq!(
        message("read \"a\""),
        "in `id` of `read _`: found a string; expected a `File Id`, written with its name: `File Id …`",
    );
    assert_eq!(
        message("feel (`calm), read \"a\""),
        "in `id` of `read _`: found a string; expected a `File Id`, written with its name: `File Id …`",
        "the whole request is invalid, so none of its calls run"
    );
}

#[test]
fn a_record_a_variant_a_boolean_and_a_duration_have_types_of_their_own() {
    assert_eq!(calls("feel (`calm)")[0].0, 8, "only `mood` has `calm`");
    assert_eq!(
        calls("move to { x 1, y 2 }")[0].0,
        9,
        "only `point` has `y`"
    );
    assert_eq!(calls("fit { x 1, label \"a\" }")[0].0, 10);
    assert_eq!(
        message("fit { x 1 }"),
        "in `size` of `fit _`: `{ x 1 }` has no type of its own; annotate it, as in `fit ({ x 1 } AS: Size)`",
        "`size` and `width` both fit; `point` needs a `y`"
    );
    assert_eq!(
        calls("show (`true)"),
        [(
            11,
            vec![Value::Variant {
                shape: "true".into(),
                arguments: vec![]
            }]
        )]
    );
    assert_eq!(calls("wait (1 hours)")[0].0, 12);
}

#[test]
fn a_call_no_overload_fits_lists_them() {
    assert_eq!(
        message("tag [1.5]"),
        "found `tag _` with an argument of the type `LIST Float`; expected the types of one of its overloads: `tag (names: LIST Text)`, `tag (ids: LIST Integer)`; a value of `Bytes` or `Instant` is written like a string, so annotate it, as in `(\"AQI=\" AS: Bytes)`"
    );
}

#[test]
fn an_annotation_is_a_checked_claim() {
    assert_eq!(
        message("tag ([1] AS: LIST Text)"),
        "in `names` of `tag _`: found the number `1`; expected a string, as in `\"text\"`"
    );
    assert_eq!(
        message("feel (`calm AS: Shade)"),
        "in `mood` of `feel _`: found `AS: Shade`; expected `AS: Mood`, the type of `mood`"
    );
    assert_eq!(
        message("feel (`calm AS: weather)"),
        "found `weather` after `AS`, which names no type in this protocol; expected a plain type name such as `Text` or `File Id`, or a generic type such as `LIST Text`"
    );
    assert_eq!(
        message("tag ([] AS: LIST: TEXT)"),
        "found the word `TEXT`; expected the word `LIST`, `MAP`, the call `_ WHERE _` or a call whose name has no keyword; the built-in type `Text` is written in Title Case"
    );
    assert_eq!(
        message("tag [(\"a\" AS Text)]"),
        "found an annotation `AS` inside a value; expected one only around a whole argument of a call, as in `tag ([] AS: LIST Text)`; a value inside another is read by the type around it"
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
            arguments: vec![file_id("a")],
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
        "tag ([] AS: LIST Text),
tag [1],
open (File Id \"a\"),
scale 3.0,
paint (`dark AS Shade),
feel (`calm),
"
    );
    assert_eq!(decode_request(&schema, &request).unwrap(), calls);
}

#[test]
fn undone_by_names_one_overload() {
    let schema = schema(
        "PROTOCOL a VERSION 1,
NAMED TYPE File Id (Text),
METHOD hide (path: Text) RETURNS NOTHING ERRORS [] UNDONE BY show (path: Text),
METHOD hide (id: File Id) RETURNS NOTHING ERRORS [] UNDONE BY show (id: File Id),
METHOD show (path: Text) RETURNS NOTHING ERRORS [],
METHOD show (id: File Id) RETURNS NOTHING ERRORS [],",
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
            text: "show (id: File Id)".to_string(),
        })
    );
}

#[test]
fn an_example_annotates_as_a_request_does() {
    let parsed = anyx_protocol::Schema::parse(
        "PROTOCOL a VERSION 1, NAMED TYPE File Id (Text),
METHOD read (id: File Id) RETURNS NOTHING ERRORS [] EXAMPLES [read (File Id \"a\") RETURNS NOTHING],",
    );
    assert!(parsed.is_ok(), "{parsed:?}");
    let unannotated = common::diagnostics(
        "PROTOCOL a VERSION 1, NAMED TYPE File Id (Text),
METHOD read (id: File Id) RETURNS NOTHING ERRORS [] EXAMPLES [read \"a\" RETURNS NOTHING],",
    );
    assert!(
        unannotated.contains(
            "in an example of `read _`: in `id` of `read _`: found a string; expected a `File Id`, written with its name: `File Id …`"
        ),
        "{unannotated}"
    );
}

/// JSON can't tell `3` from `3.0`, nor a list of `Text` from one of `Integer`, so it
/// names an overload by its typed name, `scale (Float)`.
#[cfg(feature = "json")]
#[test]
fn json_names_an_overload_with_its_parameters_types() {
    use anyx_protocol::json::{decode_request_json, encode_request_json};
    let schema = schema(TAGS);
    assert_eq!(
        encode_request_json(&schema, "scale (Float)", "[3]").unwrap(),
        "scale 3.0\n"
    );
    assert_eq!(
        encode_request_json(&schema, "scale (Integer)", "[3]").unwrap(),
        "scale 3\n"
    );
    assert_eq!(
        encode_request_json(&schema, "open (File Id)", r#"[{"FileId": "a"}]"#).unwrap(),
        "open (File Id \"a\")\n"
    );
    assert_eq!(
        encode_request_json(&schema, "scale _", "[3]").unwrap_err(),
        "found the method `scale _`, which has overloads; expected one of them by its name with its parameters' types: `scale (Float)`, `scale (Integer)`"
    );
    // A method with no overloads goes by its full name, or its typed one.
    assert_eq!(
        encode_request_json(&schema, "read _", r#"[{"FileId": "a"}]"#).unwrap(),
        "read (File Id \"a\")\n"
    );
    assert_eq!(
        encode_request_json(&schema, "read (File Id)", r#"[{"FileId": "a"}]"#).unwrap(),
        "read (File Id \"a\")\n"
    );
    assert_eq!(
        decode_request_json(&schema, "tag ([] AS: LIST Integer), read (File Id \"a\")").unwrap(),
        r#"[{"arguments":[[]],"method":"tag (LIST Integer)"},{"arguments":[{"FileId":"a"}],"method":"read _"}]"#
    );
}

#[test]
fn the_guide_lists_each_shape_s_overloads_together() {
    let guide = anyx_protocol::guide(&schema(TAGS), "");
    assert!(
        guide.contains(
            "argument whose value doesn't show its type (see Values):

- `tag (names: LIST Text)`, `tag (ids: LIST Integer)`
- `open (path: Text)`, `open (id: File Id)`
- `scale (factor: Float)`, `scale (times: Integer)`
- `paint (shade: Shade)`, `paint (colour: Colour)`
"
        ),
        "{guide}"
    );
    assert!(guide.contains("`tag ([] AS: LIST Text)`"), "{guide}");
}
