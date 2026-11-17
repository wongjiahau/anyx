//! Named types: `TYPE file id: Text` gives a type a name of its own, and is
//! written on the wire exactly like the type underneath.

mod common;

use anyx_protocol::schema::{Type, TypeKind};
use anyx_protocol::{Call, Value, decode_request, decode_response, encode_request};
use common::{assert_snapshot, diagnostics, parse_files, render, schema};

const SHARING: &str = "PROTOCOL sharing VERSION 1,

# Which file, among every account's.
TYPE file id: Text,

TYPE tags: LIST Text,

RECORD file link { file id: file id, tags: tags },

TYPE link: file link,

METHOD share (id: file id) tagged (tags: tags) RETURNS (link: link) ERRORS [missing (id: file id)],
";

#[test]
fn a_named_type_reads_with_its_doc_and_the_type_underneath() {
    let schema = schema(SHARING);
    let file_id = &schema.types[0];
    assert_eq!(file_id.name, "file id");
    assert_eq!(
        file_id.doc.as_deref(),
        Some("Which file, among every account's.")
    );
    assert_eq!(file_id.kind, TypeKind::Distinct(Type::Text));
    assert_eq!(
        schema.types[1].kind,
        TypeKind::Distinct(Type::List(Box::new(Type::Text)))
    );
    assert_eq!(schema.types[3].kind, TypeKind::Distinct(Type::Named(2)));
    assert_eq!(schema.underlying(&Type::Named(3)), &Type::Named(2));
    assert_eq!(schema.underlying(&Type::Named(0)), &Type::Text);
    assert_eq!(schema.type_name(&Type::Named(0)), "file id");
}

#[test]
fn a_named_type_is_written_like_the_type_underneath() {
    let schema = schema(SHARING);
    let call = Call {
        method: 0,
        arguments: vec![
            Value::Text("a".to_string()),
            Value::List(vec![Value::Text("hymn".to_string())]),
        ],
    };
    let request = encode_request(&schema, std::slice::from_ref(&call)).unwrap();
    assert_eq!(
        request,
        "share (\"a\" AS file id) tagged ([\"hymn\"] AS tags)\n"
    );
    assert_eq!(decode_request(&schema, &request).unwrap(), vec![call]);
    let response = "RETURNS { file id \"a\", tags [\"hymn\"] }";
    let anyx_protocol::Response::Outcomes(outcomes) =
        decode_response(&schema, &[0], response).unwrap()
    else {
        panic!("expected outcomes");
    };
    let anyx_protocol::Outcome::Value(Value::Record(fields)) = &outcomes[0] else {
        panic!("expected a record, found {outcomes:?}");
    };
    assert_eq!(fields["file id"], Value::Text("a".to_string()));
    assert_eq!(
        decode_request(&schema, "share 3 tagged []")
            .unwrap_err()
            .to_string(),
        "1:7: in `id` of `share _ tagged _`: found the number `3`; expected a string, as in `\"text\"`"
    );
}

#[cfg(feature = "json")]
#[test]
fn a_named_type_is_the_type_underneath_in_json() {
    use anyx_protocol::json::{decode_response_json, encode_request_json};
    let schema = schema(SHARING);
    assert_eq!(
        encode_request_json(&schema, "share _ tagged _", r#"["a", ["hymn"]]"#).unwrap(),
        "share (\"a\" AS file id) tagged ([\"hymn\"] AS tags)\n"
    );
    assert_eq!(
        decode_response_json(&schema, "share _ tagged _", "ERROR missing \"a\"").unwrap(),
        r#"{"error":{"id":"a","kind":"missing"},"kind":"error"}"#
    );
    assert_eq!(
        decode_response_json(
            &schema,
            "share _ tagged _",
            "RETURNS { file id \"a\", tags [] }"
        )
        .unwrap(),
        r#"{"kind":"value","value":{"fileId":"a","tags":[]}}"#
    );
}

#[test]
fn the_guide_names_each_named_type_and_the_type_underneath() {
    let guide = anyx_protocol::guide(&schema(SHARING), "");
    assert!(
        guide.contains(
            "- A named TYPE is written exactly like the type it names, but holds
  only what its name says:
  - `file id`: `Text`. Which file, among every account's.
  - `tags`: `LIST Text`.
  - `link`: `file link`.
"
        ),
        "{guide}"
    );
    assert!(
        guide.contains("    share (\"a\" AS: file id) tagged ([…] AS: tags)\n"),
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
            "PUBLIC TYPE file id: Text, TYPE secret: Text,",
        ),
        (
            "files.protocol.anyx",
            "USE \"./common.protocol.anyx\", METHOD read (id: file id) RETURNS (text: Text),",
        ),
    ];
    let schema = parse_files(&files).unwrap_or_else(|found| panic!("{}", render(&files, &found)));
    let read = schema.method("read _").unwrap();
    assert_eq!(
        schema.type_name(&read.shape.parameters[0].r#type),
        "file id"
    );
    let guide = anyx_protocol::guide(&schema, "");
    assert!(guide.contains("  - `file id`: `Text`.\n"), "{guide}");
    let files = [
        files[0],
        files[1],
        (
            "files.protocol.anyx",
            "USE \"./common.protocol.anyx\", METHOD read (id: secret) RETURNS (text: Text),",
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
            "a named type referring to itself",
            "PROTOCOL a VERSION 1, TYPE a: LIST: a, TYPE b: c, TYPE c: b,",
        ),
        (
            "a named type holding a record that holds it",
            "PROTOCOL a VERSION 1, RECORD r { x: a }, TYPE a: r,",
        ),
        (
            "a named type defined twice",
            "PROTOCOL a VERSION 1, TYPE a: Text, RECORD a { b: Text },",
        ),
        (
            "a named type over an unknown type",
            "PROTOCOL a VERSION 1, TYPE a: thing,",
        ),
        (
            "a named type over an optional type",
            "PROTOCOL a VERSION 1, TYPE a: Text DEFAULT \"\",",
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
fn a_named_type_may_be_an_instant_a_map_or_have_a_default() {
    let schema = schema(
        "PROTOCOL visits VERSION 1,
TYPE opened at: Instant,
TYPE openings: MAP: opened at,
TYPE label: Text,
RECORD visit { at: opened at, by person: openings, label: label DEFAULT \"none\" },
METHOD visits RETURNS (visits: LIST: visit),
",
    );
    let response =
        "RETURNS [{ at \"1970-01-01T00:00:01Z\", by person { \"ann\" \"1970-01-01T00:00:02Z\" } }]";
    let anyx_protocol::Response::Outcomes(outcomes) =
        decode_response(&schema, &[0], response).unwrap()
    else {
        panic!("expected outcomes");
    };
    let anyx_protocol::Outcome::Value(Value::List(visits)) = &outcomes[0] else {
        panic!("expected a list, found {outcomes:?}");
    };
    let Value::Record(fields) = &visits[0] else {
        panic!("expected a record, found {visits:?}");
    };
    assert_eq!(fields["label"], Value::Text("none".to_string()));
    assert!(matches!(fields["at"], Value::Instant(_)));
    assert_eq!(
        diagnostics(
            "PROTOCOL visits VERSION 1,
TYPE label: Text,
RECORD visit { label: label DEFAULT 3 },
"
        ),
        "schema:3:37: in the `DEFAULT` of `label`: found the number `3`; expected a string, as in `\"text\"`\n  RECORD visit { label: label DEFAULT 3 },\n                                      ^"
    );
}
