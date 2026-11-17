use anyx_protocol::{FieldsBuilder, Schema, Value, encode_value, schema::Type};

#[test]
fn values_are_written_without_a_response_envelope() {
    let schema = Schema::parse("PROTOCOL example VERSION 1, RECORD Entry { name: Text }, METHOD entries RETURNS (entries: LIST Entry)").unwrap();
    let value = Value::List(vec![Value::Record(
        FieldsBuilder::new()
            .field("name", "hello\n世界".to_string())
            .build(),
    )]);
    let expected = &schema.methods[0].returns.as_ref().unwrap().r#type;
    let source = encode_value(&schema, expected, &value).unwrap();
    let parsed = anyx::parse(&source);
    assert!(parsed.errors.is_empty());
    assert!(matches!(
        parsed.syntax.nodes()[0].kind,
        anyx::NodeKind::Brackets(_)
    ));
}

#[test]
fn rejects_values_that_do_not_fit_the_type() {
    let schema = Schema::parse("PROTOCOL example VERSION 1, RECORD Entry { name: Text }, METHOD entry RETURNS (entry: Entry)").unwrap();
    let expected = &schema.methods[0].returns.as_ref().unwrap().r#type;
    assert!(encode_value(&schema, &Type::Integer, &Value::Text("wrong".into())).is_err());
    assert!(encode_value(&schema, expected, &Value::Record(Default::default())).is_err());
    assert!(encode_value(&schema, &Type::Float, &Value::Float(f64::NAN)).is_err());
    assert!(encode_value(&schema, &Type::Text, &Value::Nothing).is_err());
}

#[test]
fn standalone_arguments_synthesize_types_without_a_method() {
    use anyx_protocol::{decode_argument, decode_type, decode_value};
    let schema = Schema::parse("PROTOCOL example VERSION 1, TYPE File Id: Text").unwrap();
    assert_eq!(
        decode_argument(&schema, "42").unwrap(),
        (Type::Integer, Value::Integer(42))
    );
    let named = decode_type(&schema, "File Id").unwrap();
    assert_eq!(
        decode_argument(&schema, "(\"path\" AS: File Id)").unwrap(),
        (named, Value::Text("path".into()))
    );
    assert!(decode_argument(&schema, "[]").is_err());
    assert_eq!(
        decode_value(&schema, &Type::List(Box::new(Type::Text)), "[]").unwrap(),
        Value::List(vec![])
    );
    assert!(decode_value(&schema, &Type::Integer, "\"wrong\"").is_err());
    assert!(decode_type(&schema, "Unknown Type").is_err());
}

#[test]
fn wire_requests_still_accept_values_rather_than_executable_expressions() {
    let schema =
        Schema::parse("PROTOCOL example VERSION 1, METHOD read (name: Text) RETURNS (value: Text)")
            .unwrap();
    assert!(
        anyx_protocol::decode_request(&schema, "read (environment / read variable \"HOME\")")
            .is_err()
    );
}
