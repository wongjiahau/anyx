use anyx_protocol::schema::Type;
use anyx_protocol::{
    Call, Outcome, Response, Schema, Value, decode_request, decode_response, encode_request,
    encode_response,
};

const SOURCE: &str = r#"PROTOCOL checked VERSION 1,
TYPE ALIAS Revision: Integer . WHERE [1 <= _],
TYPE ALIAS Probability: Float . WHERE [0 <= _ <= 1],
TYPE ALIAS Score Path: Text . WHERE [_ ends with ".poentaro"],
TYPE ALIAS Limit: (Integer) WHERE [10 <= _ < 30],
TYPE ALIAS Short Text: Text . WHERE [1 <= (_ length) <= 3],
TYPE Summary { path: Score Path, revision: Revision, limit (Limit) DEFAULT limit 10 },
METHOD write (path: Score Path) at (revision: Revision) RETURNS (revision: Revision) ERRORS [| |],
METHOD limit (value: Integer . WHERE [10 <= _ < 30]) RETURNS (value: Integer . WHERE [10 <= _ < 30]) ERRORS [| |],
METHOD summary (value: Summary) RETURNS (value: Summary) ERRORS [| |],
METHOD label (text: Short Text) RETURNS NOTHING ERRORS [| |],
"#;
fn schema() -> Schema {
    Schema::parse(SOURCE).unwrap()
}
/// The type `name` stands for in `source`, read through a method that
/// takes it, since an alias makes no type of its own.
fn ty(source: &str, name: &str) -> Type {
    let source = source.trim_end().trim_end_matches(',');
    let probed = Schema::parse(&format!(
        "{source}, METHOD probe (value: {name}) RETURNS NOTHING ERRORS [| |]"
    ))
    .unwrap();
    probed.method("probe _").unwrap().shape.parameters[0]
        .r#type
        .clone()
}

#[test]
fn both_spellings_decode_to_the_same_refinement_and_annotations_round_trip() {
    let schema = schema();
    for n in [10, 29] {
        let calls = vec![Call {
            method: 1,
            arguments: vec![Value::Integer(n)],
        }];
        let wire = encode_request(&schema, &calls).unwrap();
        assert_eq!(decode_request(&schema, &wire).unwrap(), calls, "{wire}");
    }
    let a = Schema::parse("TYPE ALIAS Limit: Integer . WHERE [10 <= _ < 30]").unwrap();
    let b = Schema::parse("TYPE ALIAS Limit: (Integer) WHERE [10 <= _ < 30]").unwrap();
    assert_eq!(a.types[0].kind, b.types[0].kind);
    let formatted = anyx_protocol::format_schema(SOURCE).unwrap();
    assert_eq!(Schema::parse(&formatted).unwrap().types, schema.types);
}

#[test]
fn failed_bound_names_value_type_predicate_argument_call_and_location() {
    let schema = schema();
    let error = decode_request(
        &schema,
        "write \"a.poentaro\" at 1,\nwrite \"b.poentaro\" at 0",
    )
    .unwrap_err();
    assert_eq!(error.line, 2);
    for text in ["revision", "write _ at _", "`0`", "1 <= _", "at least 1"] {
        assert!(error.message.contains(text), "{error}");
    }
    for (n, expected) in [
        (9, "at least 10"),
        (30, "less than 30"),
        (250, "less than 30"),
    ] {
        let error = decode_request(
            &schema,
            &format!("limit ({n} AS: (Integer) WHERE [10 <= _ < 30])"),
        )
        .unwrap_err();
        assert!(error.message.ends_with(expected), "{error}");
    }
}

#[test]
fn nested_fields_lists_maps_and_defaults_enforce_refinements() {
    let schema = schema();
    let error = decode_request(&schema, "summary { path \"bad.txt\", revision 1 }").unwrap_err();
    assert!(error.message.contains("path"));
    assert!(error.message.contains("ends with"));
    assert!(error.message.contains("bad.txt"));
    let calls = decode_request(&schema, "summary { path \"a.poentaro\", revision 1 }").unwrap();
    let Value::Record(fields) = &calls[0].arguments[0] else {
        panic!()
    };
    assert_eq!(fields["limit"], Value::Integer(10));
    for source in [
        "TYPE ALIAS Positive: Integer . WHERE [0 < _], TYPE Row { xs: LIST Positive }, METHOD read (r: Row) RETURNS NOTHING ERRORS [| |]",
        "TYPE ALIAS Positive: Integer . WHERE [0 < _], TYPE Row { xs: MAP Positive }, METHOD read (r: Row) RETURNS NOTHING ERRORS [| |]",
    ] {
        let s = Schema::parse(source).unwrap();
        let wire = if source.contains("LIST") {
            "read { xs [1, 0] }"
        } else {
            "read { xs { \"bad\" 0 } }"
        };
        assert!(decode_request(&s, wire).is_err());
    }
    let invalid = Schema::parse(
        "TYPE ALIAS Limit: Integer . WHERE [10 <= _ < 30], TYPE Row { limit (Limit) DEFAULT limit 30 }",
    )
    .unwrap_err();
    assert!(
        invalid.iter().any(|e| e.message.contains("less than 30")),
        "{invalid:?}"
    );
}

#[test]
fn response_and_outgoing_request_values_are_checked() {
    let schema = schema();
    let calls = vec![Call {
        method: 0,
        arguments: vec![Value::Text("a.poentaro".into()), Value::Integer(1)],
    }];
    assert!(
        encode_response(
            &schema,
            &calls,
            &Response::Outcomes(vec![Outcome::Value(Value::Integer(0))])
        )
        .is_err()
    );
    let wire =
        anyx_protocol::server::respond(&schema, &calls, vec![Outcome::Value(Value::Integer(0))]);
    assert_eq!(
        decode_response(&schema, &[0], &wire).unwrap(),
        Response::Outcomes(vec![Outcome::InternalError])
    );
    assert!(decode_response(&schema, &[0], "RETURNS 0").is_err());
    let bad = vec![Call {
        method: 0,
        arguments: vec![Value::Text("bad.txt".into()), Value::Integer(1)],
    }];
    assert!(
        encode_request(&schema, &bad)
            .unwrap_err()
            .message
            .contains("ends with")
    );
}

#[test]
fn predicates_are_typed_linear_and_do_not_guess_operator_precedence() {
    for body in [
        "Integer . WHERE [_ ends with \"x\"]",
        "Text . WHERE [0 < _]",
        "Integer . WHERE [0 < _ <= 1 + 2]",
        "Integer . WHERE [(_ * _) < 20]",
        "Integer . WHERE [1 < 2]",
        "Integer . WHERE [0.5 < _]",
        "Integer . WHERE [(_ length) < 4]",
    ] {
        assert!(
            Schema::parse(&format!("TYPE ALIAS Bad {body}")).is_err(),
            "{body}"
        );
    }
    let s_source = "TYPE ALIAS Linear: Integer . WHERE [(_ + 1) <= 30, 0 < (_ * 2)], TYPE ALIAS Large: Integer . WHERE [9007199254740992 < _], TYPE ALIAS Ends: Text . WHERE [_ starts with \"a\", _ contains \"b\", _ ends with \"c\"]";
    let s = Schema::parse(s_source).unwrap();
    for n in [1, 29] {
        assert!(
            anyx_protocol::refinement::validate(&s, &ty(s_source, "Linear"), &Value::Integer(n))
                .is_ok()
        );
    }
    for n in [0, 30] {
        assert!(
            anyx_protocol::refinement::validate(&s, &ty(s_source, "Linear"), &Value::Integer(n))
                .is_err()
        );
    }
    assert!(
        anyx_protocol::refinement::validate(
            &s,
            &ty(s_source, "Large"),
            &Value::Integer(9007199254740993)
        )
        .is_ok()
    );
    assert!(
        anyx_protocol::refinement::validate(
            &s,
            &ty(s_source, "Large"),
            &Value::Integer(9007199254740992)
        )
        .is_err()
    );
    assert!(
        anyx_protocol::refinement::validate(&s, &ty(s_source, "Ends"), &Value::Text("abc".into()))
            .is_ok()
    );
}

#[test]
fn lengths_count_unicode_scalars_and_floats_check_both_bounds() {
    let s = schema();
    let s_source = SOURCE;
    for text in ["a", "😀", "中文語"] {
        assert!(
            anyx_protocol::refinement::validate(
                &s,
                &ty(s_source, "Short Text"),
                &Value::Text(text.into())
            )
            .is_ok()
        );
    }
    for text in ["", "abcd"] {
        assert!(
            anyx_protocol::refinement::validate(
                &s,
                &ty(s_source, "Short Text"),
                &Value::Text(text.into())
            )
            .is_err()
        );
    }
    for n in [0.0, 0.5, 1.0] {
        assert!(
            anyx_protocol::refinement::validate(&s, &ty(s_source, "Probability"), &Value::Float(n))
                .is_ok()
        );
    }
    for n in [-0.1, 1.1, f64::NAN, f64::INFINITY] {
        assert!(
            anyx_protocol::refinement::validate(&s, &ty(s_source, "Probability"), &Value::Float(n))
                .is_err()
        );
    }
}

#[test]
fn guide_describes_the_predicates_in_words() {
    let guide = anyx_protocol::guide(&schema(), "");
    for text in [
        "at least 1",
        "at least 10 and less than 30",
        "ends with",
        "length at least 1",
    ] {
        assert!(guide.contains(text), "{guide}");
    }
}

#[cfg(feature = "json")]
#[test]
fn json_preserves_primitives_and_reports_the_same_predicate_errors() {
    use anyx_protocol::json::{from_json, to_json};
    let s = schema();
    let s_source = SOURCE;
    let revision = ty(s_source, "Revision");
    assert_eq!(
        from_json(&s, &revision, &serde_json::json!(1)).unwrap(),
        Value::Integer(1)
    );
    assert_eq!(
        to_json(&s, &revision, &Value::Integer(1)),
        serde_json::json!(1)
    );
    let error = from_json(&s, &revision, &serde_json::json!(0)).unwrap_err();
    for text in ["`0`", "1 <= _", "at least 1"] {
        assert!(error.contains(text), "{error}");
    }
    let error = from_json(
        &s,
        &ty(s_source, "Summary"),
        &serde_json::json!({"path":"bad.txt","revision":1}),
    )
    .unwrap_err();
    assert!(error.contains("path"));
    assert!(error.contains("ends with"));
}

#[test]
fn refined_named_types_cannot_hide_recursive_cycles() {
    for source in [
        "TYPE ALIAS Loop: Loop . WHERE [0 < _]",
        "TYPE Loop { next: Loop . WHERE [0 < _] }",
    ] {
        assert!(Schema::parse(source).is_err());
    }
    let s_source = "TYPE ALIAS First: Second . WHERE [1 <= _], TYPE ALIAS Second: Integer";
    let s = Schema::parse(s_source).unwrap();
    assert!(
        anyx_protocol::refinement::validate(&s, &ty(s_source, "First"), &Value::Integer(0))
            .is_err()
    );
}

#[test]
fn imports_keep_predicate_diagnostic_locations_and_aliased_refinements() {
    let library = "PUBLIC TYPE ALIAS Bad: Text . WHERE [0 < _]";
    let errors = Schema::load("main.protocol.anyx", |path| -> Result<String, String> {
        Ok(match path {
            "main.protocol.anyx" => "PROTOCOL main VERSION 1, USE \"library.protocol.anyx\"",
            "library.protocol.anyx" => library,
            _ => return Err(path.into()),
        }
        .into())
    })
    .unwrap_err();
    assert_eq!(errors[0].path, "library.protocol.anyx");
    assert_eq!(&library[errors[0].span.start..errors[0].span.end], "0 < _");
    let schema = Schema::load("main.protocol.anyx", |path| -> Result<String,String> {
        Ok(match path { "main.protocol.anyx" => "PROTOCOL main VERSION 1, USE \"library.protocol.anyx\", METHOD set (revision: Revision) RETURNS NOTHING ERRORS [| |]", "library.protocol.anyx" => "PUBLIC TYPE ALIAS Revision: Integer . WHERE [1 <= _]", _ => return Err(path.into()) }.into())
    }).unwrap();
    assert!(
        decode_request(&schema, "set 0")
            .unwrap_err()
            .message
            .contains("at least 1")
    );
}

#[test]
fn inline_refinements_work_inside_generic_arguments_and_container_constraints() {
    let schema_source = "TYPE Box of (Item: TYPE) { value: Item }, TYPE ALIAS Batch: (LIST Integer) WHERE [1 <= (_ length) <= 3], METHOD boxed (value: Box of (Integer . WHERE [10 <= _ < 30])) RETURNS NOTHING ERRORS [| |], METHOD batch (value: Batch) RETURNS NOTHING ERRORS [| |]";
    let schema = Schema::parse(schema_source).unwrap();
    let call = Call {
        method: 0,
        arguments: vec![Value::Record([("value".into(), Value::Integer(10))].into())],
    };
    let wire = encode_request(&schema, std::slice::from_ref(&call)).unwrap();
    assert_eq!(decode_request(&schema, &wire).unwrap(), vec![call]);
    assert!(
        decode_request(
            &schema,
            "boxed ({ value 30 } AS: Box of (Integer . WHERE [10 <= _ < 30]))"
        )
        .is_err()
    );
    assert!(decode_request(&schema, "batch [1, 2]").is_ok());
    assert!(decode_request(&schema, "batch [1, 2, 3, 4]").is_err());
    assert!(Schema::parse("METHOD set (value: Integer . WHERE [0 < _]) RETURNS NOTHING ERRORS [| |], METHOD set (value: Integer . WHERE [_ < 10]) RETURNS NOTHING ERRORS [| |]").is_err());
}

#[test]
fn guide_example_satisfies_its_parameter_refinements() {
    let schema = schema();
    let guide = anyx_protocol::guide(&schema, "");
    let example = guide
        .lines()
        .find(|line| line.starts_with("    write "))
        .unwrap()
        .trim();
    decode_request(&schema, example).unwrap_or_else(|e| panic!("{example}: {e}"));
}

#[test]
fn integer_overflow_fails_and_container_lengths_use_their_actual_units() {
    let schema_source = "TYPE ALIAS Wide: Integer . WHERE [0 < ((_ * 9223372036854775807) * 9223372036854775807)], TYPE ALIAS Pair Of Bytes: Bytes . WHERE [(_ length) = 2], TYPE ALIAS Small Map: (MAP Integer) WHERE [(_ length) <= 1]";
    let schema = Schema::parse(schema_source).unwrap();
    assert!(
        anyx_protocol::refinement::validate(
            &schema,
            &ty(schema_source, "Wide"),
            &Value::Integer(1)
        )
        .is_ok()
    );
    assert!(
        anyx_protocol::refinement::validate(
            &schema,
            &ty(schema_source, "Wide"),
            &Value::Integer(3)
        )
        .is_err()
    );
    assert!(
        anyx_protocol::refinement::validate(
            &schema,
            &ty(schema_source, "Pair Of Bytes"),
            &Value::Bytes(vec![0, 255])
        )
        .is_ok()
    );
    assert!(
        anyx_protocol::refinement::validate(
            &schema,
            &ty(schema_source, "Pair Of Bytes"),
            &Value::Bytes(vec![0])
        )
        .is_err()
    );
    let map = Value::Map(
        [
            ("a".into(), Value::Integer(1)),
            ("b".into(), Value::Integer(2)),
        ]
        .into(),
    );
    assert!(
        anyx_protocol::refinement::validate(&schema, &ty(schema_source, "Small Map"), &map)
            .is_err()
    );
    assert!(anyx_protocol::decode_type(&schema, "Text . WHERE [0 < _]").is_err());
}

#[test]
fn inline_refinement_arguments_need_no_annotation() {
    let schema = Schema::parse(
        r#"
METHOD short (value: Text . WHERE [1 <= (_ length) <= 3]) RETURNS NOTHING ERRORS [| |],
METHOD limit (value: Integer . WHERE [10 <= _ < 30]) RETURNS NOTHING ERRORS [| |],
METHOD many (values: LIST (Integer . WHERE [0 < _])) RETURNS NOTHING ERRORS [| |],
"#,
    )
    .unwrap();
    for request in [
        r#"short "he""#,
        r#"short ("😀")"#,
        "limit 10",
        "many [1, 2]",
    ] {
        assert!(decode_request(&schema, request).is_ok(), "{request}");
    }
    for (request, bound) in [
        (r#"short "hello""#, "at most 3"),
        ("limit 30", "less than 30"),
        ("many [1, 0]", "greater than 0"),
    ] {
        let error = decode_request(&schema, request).unwrap_err();
        assert!(error.message.contains(bound), "{error}");
    }
}

#[test]
fn refinement_overloads_use_base_types() {
    let schema = Schema::parse(
        r#"
METHOD use (value: Text . WHERE [1 <= (_ length)]) RETURNS NOTHING ERRORS [| |],
METHOD use (value: Integer . WHERE [0 < _]) RETURNS NOTHING ERRORS [| |],
"#,
    )
    .unwrap();
    assert_eq!(decode_request(&schema, r#"use "he""#).unwrap()[0].method, 0);
    assert_eq!(decode_request(&schema, "use 1").unwrap()[0].method, 1);
    assert!(
        decode_request(&schema, "use 0")
            .unwrap_err()
            .message
            .contains("greater than 0")
    );
}
