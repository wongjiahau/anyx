mod common;

use anyx_protocol::{Outcome, Response, Schema, Value, decode_request, encode_response, server};

const SOURCE: &str = r#"
PROTOCOL revisions VERSION 1,
METHOD write (path: Text) at revision (revision: Integer) before (limit: Integer)
    REQUIRES [0 < revision < limit, path . ends with ".txt"]
    RETURNS (new revision: Integer)
    ENSURES [new revision =: revision + 1]
    ERRORS [conflict]
    EXAMPLES [write "a.txt" at revision 2 before 4 RETURNS 3],
METHOD ping RETURNS NOTHING,
"#;

#[test]
fn preconditions_reject_the_entire_request() {
    let schema = common::schema(SOURCE);
    let request = "ping,\nwrite \"a.txt\" at revision 4 before 4";
    let problem = decode_request(&schema, request).unwrap_err();
    assert_eq!((problem.line, problem.column), (2, 1));
    for text in [
        "write \"a.txt\" at revision 4 before 4",
        "REQUIRES [0 < revision < limit",
        "`revision` (Integer) = `4`",
        "`limit` (Integer) = `4`",
        "expected revision less than limit",
    ] {
        assert!(problem.message.contains(text), "{}", problem.message);
    }
    assert!(
        server::read_request(&schema, request)
            .unwrap_err()
            .starts_with("INVALID REQUEST")
    );
    assert!(
        decode_request(&schema, "write \"a.md\" at revision 2 before 4")
            .unwrap_err()
            .message
            .contains("expected path ends with")
    );
}

#[test]
fn producer_checks_successful_results_against_their_own_arguments() {
    let schema = common::schema(SOURCE);
    let calls = decode_request(
        &schema,
        "write \"a.txt\" at revision 2 before 4, write \"b.txt\" at revision 3 before 4",
    )
    .unwrap();
    let outcomes = vec![
        Outcome::Value(Value::Integer(3)),
        Outcome::Value(Value::Integer(3)),
    ];
    let error = server::validate_outcome(&schema, &calls[1], &outcomes[1]).unwrap_err();
    assert!(
        error
            .message
            .contains("ENSURES [new revision =: revision + 1]")
    );
    assert_eq!(
        server::respond(&schema, &calls, outcomes.clone()),
        "RETURNS 3,\nINTERNAL ERROR,\n"
    );
    assert_eq!(
        encode_response(&schema, &calls, &Response::Outcomes(outcomes)).unwrap(),
        "RETURNS 3,\nINTERNAL ERROR,\n"
    );
    let error = Outcome::Error(Value::Variant {
        shape: "conflict".into(),
        arguments: vec![],
    });
    assert_eq!(
        server::respond(&schema, &calls[..1], vec![error]),
        "ERROR conflict\n"
    );
}

#[test]
fn conditions_are_checked_in_the_schema_and_listed_in_the_guide() {
    let schema = common::schema(SOURCE);
    let guide = anyx_protocol::guide(&schema, "");
    assert!(guide.contains("REQUIRES [0 < revision < limit"));
    assert!(guide.contains("ENSURES [new revision =: revision + 1]"));
    assert!(guide.contains("the whole request gets `INVALID REQUEST`"));
    let formatted = anyx_protocol::format_schema(SOURCE).unwrap();
    assert!(formatted.contains("\n    REQUIRES"), "{formatted}");
    assert!(Schema::parse(&formatted).is_ok());
    for (old, invalid, expected) in [
        (
            "0 < revision < limit",
            "new revision > 0",
            "unknown value `new revision`",
        ),
        (
            "new revision =: revision + 1",
            "old revision > 0",
            "unknown value `old revision`",
        ),
        (
            "0 < revision < limit",
            "path < 3",
            "numeric comparisons require",
        ),
        ("0 < revision < limit", "[]", "invalid REQUIRES"),
        ("RETURNS 3]", "RETURNS 2]", "in an example"),
        (
            "at revision 2 before 4 RETURNS 3]",
            "at revision 0 before 4 RETURNS 1]",
            "in an example",
        ),
    ] {
        let errors = common::diagnostics(&SOURCE.replace(old, invalid));
        assert!(errors.contains(expected), "{errors}");
    }
}

#[test]
fn arithmetic_uses_the_call_tree_from_anyx() {
    for expression in [
        "new revision =: revision + 1 .+ 1",
        "new revision = ((revision + 1) + 1)",
    ] {
        let source = SOURCE
            .replace("new revision =: revision + 1", expression)
            .replace("before 4 RETURNS 3]", "before 4 RETURNS 4]");
        let schema = common::schema(&source);
        let calls = decode_request(&schema, "write \"a.txt\" at revision 2 before 4").unwrap();
        assert_eq!(
            server::respond(&schema, &calls, vec![Outcome::Value(Value::Integer(4))]),
            "RETURNS 4\n"
        );
    }
    let flat = SOURCE.replace(
        "new revision =: revision + 1",
        "new revision = revision + 1",
    );
    let errors = common::diagnostics(&flat);
    assert!(errors.contains("invalid ENSURES"), "{errors}");
    assert!(errors.contains("group nested calls"), "{errors}");
}

#[test]
fn lengths_and_float_arithmetic_use_the_refinement_rules() {
    let schema = common::schema(
        r#"
        METHOD copy (source text: Text) into (target: Text)
            REQUIRES [(source text . length) <= (target . length)]
            RETURNS NOTHING,
        METHOD double (input: Float) RETURNS (output: Float)
            ENSURES [output = (input * 2.0)],
    "#,
    );
    assert!(decode_request(&schema, "copy \"🎵\" into \"x\"").is_ok());
    assert!(decode_request(&schema, "copy \"🎵🎵\" into \"x\"").is_err());
    let calls = decode_request(&schema, "double 1.25").unwrap();
    assert_eq!(
        server::respond(&schema, &calls, vec![Outcome::Value(Value::Float(2.5))]),
        "RETURNS 2.5\n"
    );
}

#[cfg(feature = "json")]
#[test]
fn json_bridge_uses_the_same_contracts() {
    use anyx_protocol::json::{decode_request_json, encode_request_json, encode_response_json};

    let schema = common::schema(SOURCE);
    assert!(
        decode_request_json(&schema, "write \"a.txt\" at revision 4 before 4")
            .unwrap_err()
            .contains("REQUIRES")
    );
    assert!(
        encode_request_json(
            &schema,
            "write _ at revision _ before _",
            r#"["a.txt",4,4]"#
        )
        .unwrap_err()
        .contains("REQUIRES")
    );
    assert_eq!(
        encode_response_json(
            &schema,
            "write \"a.txt\" at revision 2 before 4",
            r#"[{"kind":"value","value":2}]"#
        )
        .unwrap(),
        "INTERNAL ERROR\n"
    );
}
