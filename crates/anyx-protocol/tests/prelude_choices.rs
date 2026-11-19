use anyx_protocol::schema::{Type, TypeKind};
use anyx_protocol::{Schema, Value};

fn variant(shape: &str, arguments: Vec<Value>) -> Value {
    Value::Variant {
        shape: shape.into(),
        arguments,
    }
}

#[test]
fn prelude_choices_are_checked_schema_definitions() {
    let schema =
        Schema::parse("RECORD Example { flag: Boolean, note (Text?) DEFAULT note (null) }")
            .unwrap();
    let boolean = schema
        .types
        .iter()
        .position(|d| d.name == "Boolean")
        .unwrap();
    let nullable = schema.types.iter().position(|d| d.name == "_ ?").unwrap();
    let TypeKind::Choice(variants) = &schema.types[boolean].kind else {
        panic!("Boolean is a choice")
    };
    assert_eq!(
        variants
            .iter()
            .map(|v| v.shape.text.as_str())
            .collect::<Vec<_>>(),
        ["false", "true"]
    );
    assert_eq!(schema.types[nullable].parameters, ["item"]);
    let TypeKind::Choice(variants) = &schema.types[nullable].kind else {
        panic!("? is a choice")
    };
    assert_eq!(
        variants[1].shape.parameters[0].r#type,
        Type::Parameter("item".into())
    );
    let TypeKind::Record(fields) = &schema.types[0].kind else {
        panic!()
    };
    assert_eq!(fields[0].r#type, Type::Named(boolean));
    assert_eq!(fields[1].r#type, Type::Generic(nullable, vec![Type::Text]));
    assert_eq!(fields[1].default, Some(variant("null", vec![])));
    assert_eq!(
        schema.files.len(),
        1,
        "the implicit prelude isn't an explicit import"
    );
}

#[test]
fn nullable_and_omittable_are_independent() {
    let schema = Schema::parse("RECORD Example { required: Text?, note (Text?) DEFAULT note (null), count (Integer) DEFAULT count 0, flag (Boolean) DEFAULT flag (false) }").unwrap();
    let decode =
        |source: &str| anyx_protocol::codec::decode_value(&schema, &Type::Named(0), source);
    assert!(decode("{}").is_err());
    let Value::Record(fields) = decode("{ required: null }").unwrap() else {
        panic!()
    };
    assert_eq!(fields["required"], variant("null", vec![]));
    assert_eq!(fields["note"], variant("null", vec![]));
    assert_eq!(fields["count"], Value::Integer(0));
    assert_eq!(fields["flag"], variant("false", vec![]));
    let Value::Record(fields) = decode("{ required: value (\"hello\"), flag: true }").unwrap()
    else {
        panic!()
    };
    assert_eq!(
        fields["required"],
        variant("value _", vec![Value::Text("hello".into())])
    );
    assert_eq!(fields["flag"], variant("true", vec![]));
    assert!(decode("{ required: value (3) }").is_err());
    assert!(decode("{ required: null, count: null }").is_err());
    assert!(Schema::parse("RECORD Example { note: OPTIONAL Text }").is_err());
}

#[test]
fn ordinary_prelude_names_cannot_be_redeclared() {
    assert!(Schema::parse("CHOICE Boolean { maybe }").is_err());
    assert!(Schema::parse("CHOICE (item: Type) ? { empty, full (item) }").is_err());
}

#[test]
fn boolean_inference_uses_ordinary_choice_ambiguity() {
    let schema =
        Schema::parse("CHOICE Another { true }, METHOD set (flag: Boolean) RETURNS NOTHING")
            .unwrap();
    assert!(anyx_protocol::codec::decode_request(&schema, "set (true)").is_err());
    assert!(anyx_protocol::codec::decode_request(&schema, "set (true AS Boolean)").is_ok());
}

#[test]
fn generic_fields_may_default_to_a_parameter_independent_variant() {
    let schema =
        Schema::parse("RECORD Box of (item: Type) { content (item?) DEFAULT content (null) }")
            .unwrap();
    let ty = Type::Generic(0, vec![Type::Text]);
    let value = anyx_protocol::codec::decode_value(&schema, &ty, "{}").unwrap();
    assert_eq!(
        value,
        Value::Record([("content".into(), variant("null", vec![]))].into())
    );
    let value =
        anyx_protocol::codec::decode_value(&schema, &ty, "{ content: value (\"hello\") }").unwrap();
    assert_eq!(
        value,
        Value::Record(
            [(
                "content".into(),
                variant("value _", vec![Value::Text("hello".into())])
            )]
            .into()
        )
    );
    assert!(
        Schema::parse("RECORD Box of (item: Type) { content (item) DEFAULT content 3 }").is_err()
    );
}

#[cfg(feature = "json")]
#[test]
fn json_uses_ordinary_choice_values_and_only_omission_uses_defaults() {
    use anyx_protocol::json::{from_json, to_json};
    use serde_json::json;
    let schema = Schema::parse(
        "RECORD Example { note (Text?) DEFAULT note (null), flag (Boolean) DEFAULT flag (false) }",
    )
    .unwrap();
    let ty = Type::Named(0);
    let value = from_json(&schema, &ty, &json!({})).unwrap();
    assert_eq!(
        to_json(&schema, &ty, &value),
        json!({ "note": {"kind": "null"}, "flag": {"kind": "false"} })
    );
    assert!(from_json(&schema, &ty, &json!({"note": null})).is_err());
    assert!(from_json(&schema, &ty, &json!({"flag": true})).is_err());
}
