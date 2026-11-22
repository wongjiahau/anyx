//! Named types: `NAMED TYPE File Id (value: Text)` makes a type of its own, which
//! no other type fits. Its name is a shape whose slots hold types, and its
//! values are written with that shape: `File Id "a"`.

mod common;

use anyx_protocol::schema::{ShapePart, Type, TypeKind};
use anyx_protocol::{Call, Value, decode_request, decode_response, encode_request};
use common::{assert_snapshot, diagnostics, parse_files, render, schema};

const SHARING: &str = "PROTOCOL sharing VERSION 1,

# Which file, among every account's.
NAMED TYPE File Id (value: Text),

TYPE ALIAS Tags: LIST Text,

TYPE ALIAS File Link: { file id (file id: File Id), tags (tags: Tags) },

METHOD share (id: File Id) tagged (tags: Tags) RETURNS (link: File Link) ERRORS [| `missing (id: File Id) |],
";

fn file_id(id: &str) -> Value {
    Value::Variant {
        shape: "File Id _".to_string(),
        arguments: vec![Value::Text(id.to_string())],
    }
}

fn word(word: &str) -> ShapePart {
    ShapePart::Word(word.to_string())
}

#[test]
fn a_named_type_reads_with_its_doc_constructor_and_slots() {
    let schema = schema(SHARING);
    let file_id = &schema.types[0];
    assert_eq!(file_id.name, "File Id");
    assert_eq!(
        file_id.doc.as_deref(),
        Some("Which file, among every account's.")
    );
    assert_eq!(
        file_id.kind,
        TypeKind::Distinct {
            constructor: vec![word("File"), word("Id"), ShapePart::Slot],
            underlying: Type::Text,
        }
    );
    // A named type stops `underlying`: it is not the type it holds.
    assert_eq!(schema.underlying(&Type::Named(0)), &Type::Named(0));
    assert_eq!(schema.type_name(&Type::Named(0)), "File Id");
}

#[test]
fn a_named_value_is_written_with_its_name() {
    let schema = schema(SHARING);
    let call = Call {
        method: 0,
        arguments: vec![
            file_id("a"),
            Value::List(vec![Value::Text("hymn".to_string())]),
        ],
    };
    let request = encode_request(&schema, std::slice::from_ref(&call)).unwrap();
    assert_eq!(request, "share (File Id \"a\") tagged [\"hymn\"]\n");
    assert_eq!(decode_request(&schema, &request).unwrap(), vec![call]);
    let response = "RETURNS { file id (File Id \"a\"), tags [\"hymn\"] }";
    let anyx_protocol::Response::Outcomes(outcomes) =
        decode_response(&schema, &[0], response).unwrap()
    else {
        panic!("expected outcomes");
    };
    let anyx_protocol::Outcome::Value(Value::Record(fields)) = &outcomes[0] else {
        panic!("expected a record, found {outcomes:?}");
    };
    assert_eq!(fields["file id"], file_id("a"));
    assert_snapshot(
        "tests/snapshots/named_value_without_its_name.txt",
        &decode_request(&schema, "share \"a\" tagged []")
            .unwrap_err()
            .to_string(),
    );
}

#[test]
fn a_named_type_may_have_several_slots_none_or_a_slot_first() {
    let schema = schema(
        "PROTOCOL shapes VERSION 1,
NAMED TYPE Range from (start: Integer) to (end: Integer),
NAMED TYPE Unit,
NAMED TYPE (value: [| `red, `blue |]) Color,
METHOD paint (range: Range from to) with (color: Color) and (unit: Unit) RETURNS NOTHING ERRORS [| |],
",
    );
    let call = Call {
        method: 0,
        arguments: vec![
            Value::Variant {
                shape: "Range from _ to _".to_string(),
                arguments: vec![Value::Integer(1), Value::Integer(3)],
            },
            Value::Variant {
                shape: "_ Color".to_string(),
                arguments: vec![Value::Variant {
                    shape: "red".to_string(),
                    arguments: vec![],
                }],
            },
            Value::Variant {
                shape: "Unit".to_string(),
                arguments: vec![],
            },
        ],
    };
    let request = encode_request(&schema, std::slice::from_ref(&call)).unwrap();
    assert_eq!(
        request,
        "paint (Range from 1 to 3) with ((`red) Color) and (Unit)\n"
    );
    assert_eq!(decode_request(&schema, &request).unwrap(), vec![call]);
}

#[test]
fn a_union_of_named_types_picks_its_member_by_name() {
    let schema = schema(
        "PROTOCOL sharing VERSION 1,
NAMED TYPE File Id (value: Text),
NAMED TYPE Share Id (value: Text),
METHOD open (id: [| File Id, Share Id |]) RETURNS NOTHING ERRORS [| |],
",
    );
    let share = Value::Variant {
        shape: "Share Id _".to_string(),
        arguments: vec![Value::Text("b".to_string())],
    };
    assert_eq!(
        decode_request(&schema, "open (Share Id \"b\")").unwrap(),
        vec![Call {
            method: 0,
            arguments: vec![share],
        }]
    );
    assert_eq!(
        decode_request(&schema, "open (File Id \"a\")").unwrap(),
        vec![Call {
            method: 0,
            arguments: vec![file_id("a")],
        }]
    );
}

#[cfg(feature = "json")]
#[test]
fn a_named_value_is_under_its_tag_in_json() {
    use anyx_protocol::json::{decode_request_json, decode_response_json, encode_request_json};
    let schema = schema(SHARING);
    assert_eq!(
        encode_request_json(
            &schema,
            "share _ tagged _",
            r#"[{"FileId": "a"}, ["hymn"]]"#
        )
        .unwrap(),
        "share (File Id \"a\") tagged [\"hymn\"]\n"
    );
    assert!(encode_request_json(&schema, "share _ tagged _", r#"["a", ["hymn"]]"#).is_err());
    assert_eq!(
        decode_response_json(
            &schema,
            "share _ tagged _",
            "ERROR `missing (File Id \"a\")"
        )
        .unwrap(),
        r#"{"error":{"id":{"FileId":"a"},"kind":"missing"},"kind":"error"}"#
    );
    assert_eq!(
        decode_response_json(
            &schema,
            "share _ tagged _",
            "RETURNS { file id (File Id \"a\"), tags [] }"
        )
        .unwrap(),
        r#"{"kind":"value","value":{"fileId":{"FileId":"a"},"tags":[]}}"#
    );
    let shapes = self::schema(
        "PROTOCOL shapes VERSION 1,
NAMED TYPE Range from (start: Integer) to (end: Integer),
NAMED TYPE Unit,
NAMED TYPE File Id (value: Text),
NAMED TYPE Share Id (value: Text),
METHOD mark (range: Range from to) and (unit: Unit) for (id: [| File Id, Share Id |]) RETURNS NOTHING ERRORS [| |],
",
    );
    let method = "mark _ and _ for _";
    let arguments = r#"[{"RangeFromTo":{"end":3,"start":1}},{"Unit":[]},{"ShareId":"b"}]"#;
    let wire = "mark (Range from 1 to 3) and (Unit) for (Share Id \"b\")\n";
    assert_eq!(
        encode_request_json(&shapes, method, arguments).unwrap(),
        wire
    );
    assert_eq!(
        decode_request_json(&shapes, wire).unwrap(),
        format!(r#"[{{"arguments":{arguments},"method":"{method}"}}]"#)
    );
    assert!(
        encode_request_json(
            &shapes,
            method,
            r#"[{"RangeFromTo":[1,3]},{"Unit":[]},{"ShareId":"b"}]"#
        )
        .is_err()
    );
}

#[test]
fn named_types_require_unique_slot_names() {
    for (declaration, expected) in [
        (
            "NAMED TYPE File Id (Text)",
            "found an unnamed slot; write `(name: Type)`",
        ),
        (
            "NAMED TYPE Span from (edge: Integer) to (edge: Integer)",
            "found a second slot named `edge`",
        ),
    ] {
        let source = format!("PROTOCOL a VERSION 1, {declaration},");
        let found = diagnostics(&source);
        assert!(found.contains(expected), "{found}");
    }
}

#[test]
fn the_guide_names_each_named_type_and_what_it_holds() {
    let guide = anyx_protocol::guide(&schema(SHARING), "");
    assert!(
        guide.contains(
            "- A NAMED TYPE is a type of its own, written with its name and a
  value in each slot, as in `File Id \"a\"`:
  - `File Id …`, holding a `Text`. Which file, among every account's.
"
        ),
        "{guide}"
    );
    assert!(
        guide.contains("    share (File Id \"a\") tagged […]\n"),
        "{guide}"
    );
}

#[test]
fn a_public_named_type_is_shared_by_a_direct_import() {
    let files = [
        (
            "protocol.protocol.anyx",
            "PROTOCOL p VERSION 1, USE \"./files.protocol.anyx\",",
        ),
        (
            "common.protocol.anyx",
            "PUBLIC NAMED TYPE File Id (value: Text), NAMED TYPE Secret (value: Text),",
        ),
        (
            "files.protocol.anyx",
            "USE \"./common.protocol.anyx\", METHOD read (id: File Id) RETURNS (text: Text) ERRORS [| |],",
        ),
    ];
    let schema = parse_files(&files).unwrap_or_else(|found| panic!("{}", render(&files, &found)));
    let read = schema.method("read _").unwrap();
    assert_eq!(
        schema.type_name(&read.shape.parameters[0].r#type),
        "File Id"
    );
    let files = [
        files[0],
        files[1],
        (
            "files.protocol.anyx",
            "USE \"./common.protocol.anyx\", METHOD read (id: Secret) RETURNS (text: Text) ERRORS [| |],",
        ),
    ];
    let found = parse_files(&files).unwrap_err();
    assert!(
        found
            .iter()
            .any(|diagnostic| diagnostic.message.contains("not visible")
                && diagnostic.message.contains("common.protocol.anyx"))
    );
}

#[test]
fn mistakes_in_named_types_are_diagnosed() {
    let cases = [
        (
            "a bare TYPE, which is neither an alias nor a named type",
            "PROTOCOL a VERSION 1, TYPE File Id: Text,",
        ),
        (
            "a named type holding itself",
            "PROTOCOL a VERSION 1, NAMED TYPE Alpha (value: Alpha),",
        ),
        (
            "a named type holding a record that holds it",
            "PROTOCOL a VERSION 1, TYPE ALIAS Row: { x (x: Alpha) }, NAMED TYPE Alpha (value: Row),",
        ),
        (
            "a named type defined twice",
            "PROTOCOL a VERSION 1, NAMED TYPE Alpha (value: Text), TYPE ALIAS Alpha: { b (b: Text) },",
        ),
        (
            "a named type over an unknown type",
            "PROTOCOL a VERSION 1, NAMED TYPE Alpha (value: thing),",
        ),
    ];
    let rendered: Vec<String> = cases
        .iter()
        .map(|(title, source)| format!("== {title}\n{source}\n{}\n", diagnostics(source)))
        .collect();
    assert_snapshot(
        "tests/snapshots/named_type_diagnostics.txt",
        &rendered.join("\n"),
    );
}

#[test]
fn a_named_type_may_hold_itself_through_a_list() {
    let schema = schema(
        "PROTOCOL trees VERSION 1,
NAMED TYPE Tree (value: LIST Tree),
METHOD plant (tree: Tree) RETURNS NOTHING ERRORS [| |],
",
    );
    let leaf = Value::Variant {
        shape: "Tree _".to_string(),
        arguments: vec![Value::List(vec![])],
    };
    let call = Call {
        method: 0,
        arguments: vec![Value::Variant {
            shape: "Tree _".to_string(),
            arguments: vec![Value::List(vec![leaf])],
        }],
    };
    let request = encode_request(&schema, std::slice::from_ref(&call)).unwrap();
    assert_eq!(decode_request(&schema, &request).unwrap(), vec![call]);
}
