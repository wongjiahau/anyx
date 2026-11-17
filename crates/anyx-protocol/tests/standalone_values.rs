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
