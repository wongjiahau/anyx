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
        "REQUIRES failed for `write (path: Text) at revision (revision: Integer) before (limit: Integer)`",
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

#[test]
fn prelude_boolean_functions_compose_with_typed_predicates() {
    let schema = common::schema(
        r#"
        METHOD admit (start: Integer) before (end: Integer) when (enabled: Boolean)
            REQUIRES [(start < end) and: enabled]
            RETURNS (admitted: Boolean)
            ENSURES [(not (not (admitted))) xor: false],
        METHOD match (text: Text) with (prefix: Text) or (allowed: Boolean)
            REQUIRES [(text . starts with (prefix)) or: allowed]
            RETURNS NOTHING,
        "#,
    );
    let calls = decode_request(&schema, "admit 1 before 2 when (true)").unwrap();
    assert!(decode_request(&schema, "admit 2 before 1 when (true)").is_err());
    assert!(decode_request(&schema, "admit 1 before 2 when (false)").is_err());
    assert_eq!(
        server::respond(
            &schema,
            &calls,
            vec![Outcome::Value(anyx_protocol::IntoValue::into_value(false))]
        ),
        "INTERNAL ERROR\n"
    );
    assert!(decode_request(&schema, r#"match "abc" with "a" or (false)"#).is_ok());
    assert!(decode_request(&schema, r#"match "abc" with "z" or (true)"#).is_ok());
    assert!(decode_request(&schema, r#"match "abc" with "z" or (false)"#).is_err());
}

#[test]
fn list_tests_rule_out_repeats_and_contradicting_settings() {
    let schema = common::schema(
        r#"
        CHOICE Setting { any case, smart case, hidden, (encoding: Text) encoding },
        METHOD find (pattern: Text) with (settings: LIST Setting)
            REQUIRES [
                settings . has no repeats,
                settings . has at most one of [any case, smart case],
                settings . has at most one of [_ encoding],
            ]
            RETURNS NOTHING,
        "#,
    );
    for accepted in [
        r#"find "a" with ([] AS: LIST Setting)"#,
        r#"find "a" with [any case, hidden]"#,
        r#"find "a" with ["utf-8" encoding, smart case]"#,
    ] {
        if let Err(error) = decode_request(&schema, accepted) {
            panic!("{accepted}: {error}");
        }
    }
    for (rejected, expected) in [
        (
            r#"find "a" with [hidden, hidden]"#,
            "settings has no repeats",
        ),
        (
            r#"find "a" with [any case, smart case]"#,
            "settings has at most one of [any case, smart case]",
        ),
        (
            r#"find "a" with ["utf-8" encoding, "latin1" encoding]"#,
            "settings has at most one of [_ encoding]",
        ),
    ] {
        let error = decode_request(&schema, rejected).unwrap_err().to_string();
        assert!(error.contains(expected), "{rejected}: {error}");
    }
}

#[test]
fn list_tests_name_variants_of_a_choice_in_a_method_condition() {
    for (source, expected) in [
        (
            "CHOICE Case { any case, smart case }, METHOD f (cases: LIST Case) REQUIRES [cases . has at most one of [exact case]] RETURNS NOTHING",
            "`exact case` is not a variant of Case",
        ),
        (
            "METHOD f (names: LIST Text) REQUIRES [names . has at most one of [a]] RETURNS NOTHING",
            "tests a LIST of a CHOICE",
        ),
        (
            "METHOD f (name: Text) REQUIRES [name . has no repeats] RETURNS NOTHING",
            "`has no repeats` tests a LIST",
        ),
        (
            "CHOICE Case { any case }, METHOD f (cases: LIST Case) REQUIRES [cases . has at most one of (any case)] RETURNS NOTHING",
            "expected variants in square brackets",
        ),
        (
            "TYPE Names: LIST Text . WHERE [_ . has no repeats]",
            "are for REQUIRES and ENSURES",
        ),
    ] {
        let errors = common::diagnostics(source);
        assert!(errors.contains(expected), "{source}\n{errors}");
    }
}

#[test]
fn conditions_require_a_declared_signature() {
    for (condition, expected) in [
        ("value + 1", "a predicate must return Boolean"),
        ("not (value)", "a predicate must return Boolean"),
        ("0 < value > 2", "unsupported predicate"),
    ] {
        let errors = common::diagnostics(&format!(
            "METHOD check (value: Integer) REQUIRES [{condition}] RETURNS NOTHING"
        ));
        assert!(errors.contains(expected), "{errors}");
    }
    let errors = common::diagnostics("FUNCTION (value: Integer) is valid RETURNS (holds: Boolean)");
    assert!(errors.contains("only the protocol prelude"), "{errors}");
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
