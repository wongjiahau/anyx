//! Unions: `UNION [Integer, `none]` holds a value of any of its members,
//! written as that member writes it, so a value's form tells its member.

mod common;

use anyx_protocol::Value;
use anyx_protocol::codec::{decode_value, encode_value};
use anyx_protocol::schema::Type;
use common::{assert_snapshot, diagnostics, schema};

fn variant(shape: &str, arguments: Vec<Value>) -> Value {
    Value::Variant {
        shape: shape.into(),
        arguments,
    }
}

fn mixed() -> (anyx_protocol::Schema, Type) {
    let schema = schema(
        "PROTOCOL p VERSION 1,
        TYPE ALIAS Shade: UNION [`dark, `light],
        TYPE ALIAS Mixed: UNION [Integer, Text, Shade, `none, `custom hue (hue: Float), (Integer) Pair],
        TYPE ALIAS (Item: TYPE) Pair: LIST Item,
        METHOD f (x: Mixed) RETURNS NOTHING ERRORS [],",
    );
    let r#type = schema.method("f _").unwrap().shape.parameters[0]
        .r#type
        .clone();
    (schema, r#type)
}

#[test]
fn a_value_is_read_as_the_member_its_form_tells() {
    let (schema, r#type) = mixed();
    let read = |source: &str| decode_value(&schema, &r#type, source).unwrap();
    assert_eq!(read("3"), Value::Integer(3));
    assert_eq!(read("\"a\""), Value::Text("a".into()));
    assert_eq!(read("`dark"), variant("dark", vec![]));
    assert_eq!(read("`none"), variant("none", vec![]));
    assert_eq!(
        read("`custom hue 0.5"),
        variant("custom hue _", vec![Value::Float(0.5)])
    );
    assert_eq!(read("[]"), Value::List(vec![]));
    for value in ["3", "\"a\"", "`dark", "`none", "`custom hue 0.5", "[]"] {
        let decoded = read(value);
        let written = encode_value(&schema, &r#type, &decoded).unwrap();
        assert_eq!(read(&written), decoded, "{value} written as {written}");
    }
    // A number is the `Integer` member's to read.
    let found = decode_value(&schema, &r#type, "1.5").unwrap_err();
    assert!(
        found.message.contains("expected an Integer"),
        "{}",
        found.message
    );
    let found = decode_value(&schema, &r#type, "{}").unwrap_err();
    assert!(found.message.contains("one of"), "{}", found.message);
    assert!(decode_value(&schema, &r#type, "custom hue 3").is_err());
}

#[cfg(feature = "json")]
#[test]
fn json_writes_a_member_as_that_member_does() {
    use anyx_protocol::json::{from_json, to_json};
    use serde_json::json;
    let (schema, r#type) = mixed();
    for (value, written) in [
        (Value::Integer(3), json!(3)),
        (Value::Text("a".into()), json!("a")),
        (variant("light", vec![]), json!({"kind": "light"})),
        (variant("none", vec![]), json!({"kind": "none"})),
        (
            variant("custom hue _", vec![Value::Float(0.5)]),
            json!({"kind": "customHue", "hue": 0.5}),
        ),
    ] {
        assert_eq!(to_json(&schema, &r#type, &value), written);
        assert_eq!(from_json(&schema, &r#type, &written).unwrap(), value);
    }
    assert!(from_json(&schema, &r#type, &json!({"kind": "other"})).is_err());
    assert!(from_json(&schema, &r#type, &json!(null)).is_err());
}

#[test]
fn a_nullable_value_is_null_or_a_value_of_its_item() {
    let schema = schema(
        "PROTOCOL p VERSION 1,
        RECORD Limits { max depth (Integer?) DEFAULT max depth (`null) },
        METHOD f (limits: Limits) RETURNS NOTHING ERRORS [],",
    );
    let limits = Type::Named(limits_index(&schema));
    let read = |source: &str| decode_value(&schema, &limits, source).unwrap();
    let Value::Record(fields) = read("{ max depth 3 }") else {
        panic!("a record")
    };
    assert_eq!(fields["max depth"], Value::Integer(3));
    let Value::Record(fields) = read("{}") else {
        panic!("a record")
    };
    assert_eq!(fields["max depth"], variant("null", vec![]));
    let anyx_protocol::schema::TypeKind::Record(fields) = &schema.types[limits_index(&schema)].kind
    else {
        panic!("a record")
    };
    assert_eq!(schema.type_name(&fields[0].r#type), "Integer?");
}

fn limits_index(schema: &anyx_protocol::Schema) -> usize {
    schema
        .types
        .iter()
        .position(|definition| definition.name == "Limits")
        .unwrap()
}

#[test]
fn an_empty_union_and_nested_unions_are_allowed() {
    let schema = schema(
        "PROTOCOL p VERSION 1,
        TYPE ALIAS Nothing: UNION [],
        TYPE ALIAS Depth: Integer?,
        METHOD f (x: UNION [Nothing, Depth, UNION [Text, `none]]) RETURNS NOTHING ERRORS [],",
    );
    let r#type = &schema.method("f _").unwrap().shape.parameters[0].r#type;
    assert_eq!(r#type.members().len(), 4);
    assert_eq!(
        schema.type_name(r#type),
        "UNION [UNION [], Integer?, UNION [Text, `none]]"
    );
    assert_eq!(
        decode_value(&schema, r#type, "`none").unwrap(),
        variant("none", vec![])
    );
    assert_eq!(
        decode_value(&schema, r#type, "`null").unwrap(),
        variant("null", vec![])
    );
}

#[test]
fn a_member_fits_where_its_union_is_expected() {
    let depth = Type::Integer;
    let null =
        match &schema("PROTOCOL p VERSION 1, METHOD f (x: Integer?) RETURNS NOTHING ERRORS [],")
            .method("f _")
            .unwrap()
            .shape
            .parameters[0]
            .r#type
        {
            Type::Union(members) => members[1].clone(),
            _ => panic!("`Integer?` is a union"),
        };
    let nullable = Type::Union(vec![depth.clone(), null.clone()]);
    assert!(nullable.accepts_argument_type(&depth));
    assert!(nullable.accepts_argument_type(&null));
    assert!(nullable.accepts_argument_type(&nullable));
    assert!(!depth.accepts_argument_type(&nullable));
    assert!(!nullable.accepts_argument_type(&Type::Text));
}

#[test]
fn mistakes_in_unions_are_diagnosed() {
    let cases = [
        (
            "two members written alike",
            "PROTOCOL a VERSION 1, TYPE ALIAS Shade: UNION [`dark, `none], TYPE ALIAS Counts: UNION [Integer, Integer], TYPE ALIAS Shades: UNION [Shade, `none], TYPE ALIAS Blobs: UNION [Text, Bytes],",
        ),
        (
            "two members written alike only in JSON",
            "PROTOCOL a VERSION 1, TYPE ALIAS Waits: UNION [Integer, Duration], RECORD Row { a: Text }, TYPE ALIAS Texts: MAP Text, TYPE ALIAS Rows: UNION [Row, Texts],",
        ),
        (
            "a nullable nullable, and a nullable choice with null",
            "PROTOCOL a VERSION 1, TYPE ALIAS Depth: Integer?, TYPE ALIAS Gap: UNION [`null, `some], METHOD f (x: Depth?) g (y: Gap?) RETURNS NOTHING ERRORS [],",
        ),
        (
            "a repeat made by a generic type's argument",
            "PROTOCOL a VERSION 1, RECORD Box of (Item: TYPE) { value: Item? }, METHOD f (x: Box of (Text?)) RETURNS NOTHING ERRORS [],",
        ),
        (
            "a list or a map as a member",
            "PROTOCOL a VERSION 1, TYPE ALIAS Paths: UNION [LIST Text, `none],",
        ),
    ];
    let rendered: Vec<String> = cases
        .iter()
        .map(|(title, source)| format!("== {title}\n{source}\n{}\n", diagnostics(source)))
        .collect();
    assert_snapshot(
        "tests/snapshots/union_diagnostics.txt",
        &rendered.join("\n"),
    );
}
