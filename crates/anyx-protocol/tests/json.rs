//! Values as JSON, for the TypeScript client.

mod common;

use anyx_protocol::json::{
    decode_request_json, decode_response_json, encode_request_json, encode_response_json,
};
use common::poentaro;

const WRITE: &str = "write _ at revision _ content _";

#[test]
fn a_request_is_written_from_json_arguments() {
    let schema = poentaro();
    let request = encode_request_json(&schema, WRITE, r#"["a", 3, "x"]"#).unwrap();
    assert_eq!(request, "write \"a\" at revision 3 content \"x\"\n");
    let mistake = encode_request_json(&schema, "read _", "[3]").unwrap_err();
    assert_eq!(mistake, "in `path`: found 3; expected a string");
}

#[test]
fn a_request_reads_back_as_json_calls() {
    let schema = poentaro();
    let request = encode_request_json(&schema, WRITE, r#"["a", 3, "x\ny"]"#).unwrap();
    assert_eq!(
        decode_request_json(&schema, &request).unwrap(),
        r#"[{"arguments":["a",3,"x\ny"],"method":"write _ at revision _ content _"}]"#
    );
    assert!(decode_request_json(&schema, "nonsense").is_err());
}

#[test]
fn a_response_is_written_from_json_outcomes() {
    let schema = poentaro();
    let request = encode_request_json(&schema, WRITE, r#"["a", 3, "x"]"#).unwrap();
    for outcome in [
        r#"{"kind":"value","value":4}"#,
        r#"{"error":{"content":"y","kind":"conflictAtRevisionContent","revision":4},"kind":"error"}"#,
        r#"{"kind":"internalError"}"#,
    ] {
        let response = encode_response_json(&schema, &request, &format!("[{outcome}]")).unwrap();
        assert_eq!(
            decode_response_json(&schema, WRITE, &response).unwrap(),
            outcome
        );
    }
    let mistake = encode_response_json(&schema, &request, r#"[{"kind":"value","value":"4"}]"#);
    assert_eq!(mistake.unwrap_err(), "found \"4\"; expected a whole number");
}

#[test]
fn each_outcome_reads_as_json() {
    let schema = poentaro();
    let cases = [
        (
            "list files",
            "list files RETURNS [{ path \"a\", revision 1 }]",
            r#"{"kind":"value","value":[{"path":"a","revision":1}]}"#,
        ),
        (
            WRITE,
            "write \"a\" at revision 3 content \"x\" ERROR conflict at revision 4 content \"y\"",
            r#"{"error":{"content":"y","kind":"conflictAtRevisionContent","revision":4},"kind":"error"}"#,
        ),
        (
            "read _",
            "read \"a\" ERROR trashed",
            r#"{"error":{"kind":"trashed"},"kind":"error"}"#,
        ),
        ("read _", "read \"a\" NOT RUN", r#"{"kind":"notRun"}"#),
        (
            "read _",
            "read \"a\" INTERNAL ERROR",
            r#"{"kind":"internalError"}"#,
        ),
        (
            "read _",
            "INVALID REQUEST { line 1, column 2, message \"m\" }",
            r#"{"column":2,"kind":"invalidRequest","line":1,"message":"m"}"#,
        ),
    ];
    for (method, response, json) in cases {
        assert_eq!(
            decode_response_json(&schema, method, response).unwrap(),
            json,
            "{response}"
        );
    }
}

#[test]
fn bytes_are_base64_in_json() {
    let schema =
        common::schema("PROTOCOL a VERSION 1, METHOD upload (file: BYTES) RETURNS (file: BYTES),");
    let request = encode_request_json(&schema, "upload _", r#"["TVRoZA=="]"#).unwrap();
    assert_eq!(request, "upload \"TVRoZA==\"\n");
    assert_eq!(
        decode_response_json(
            &schema,
            "upload _",
            "upload \"TVRoZA==\" RETURNS \"TVRoZA==\""
        )
        .unwrap(),
        r#"{"kind":"value","value":"TVRoZA=="}"#
    );
    let mistake = encode_request_json(&schema, "upload _", r#"["%%"]"#).unwrap_err();
    assert_eq!(
        mistake,
        "in `file`: found \"%%\"; expected base64 in a string"
    );
}

#[test]
fn nothing_returned_is_a_value_without_a_value_in_json() {
    let schema = common::schema("PROTOCOL a VERSION 1, METHOD sign out RETURNS NOTHING,");
    let json = decode_response_json(&schema, "sign out", "sign out RETURNS NOTHING").unwrap();
    assert_eq!(json, r#"{"kind":"value"}"#);
}

#[test]
fn an_instant_is_text_and_a_duration_milliseconds_in_json() {
    let schema = common::schema(
        "PROTOCOL a VERSION 1, METHOD wait (duration: DURATION) from (start: INSTANT) RETURNS (end: INSTANT),",
    );
    let request = encode_request_json(
        &schema,
        "wait _ from _",
        r#"[5400000, "2026-11-14T06:00:08Z"]"#,
    )
    .unwrap();
    assert_eq!(
        request,
        "wait (1 hour 30 minutes) from \"2026-11-14T06:00:08.000Z\"\n"
    );
    assert_eq!(
        decode_request_json(&schema, &request).unwrap(),
        r#"[{"arguments":[5400000,"2026-11-14T06:00:08.000Z"],"method":"wait _ from _"}]"#
    );
    assert_eq!(
        decode_response_json(
            &schema,
            "wait _ from _",
            "wait (90 seconds) from \"2026-11-14T06:00:08Z\" RETURNS \"2026-11-14T07:30:08.000Z\""
        )
        .unwrap(),
        r#"{"kind":"value","value":"2026-11-14T07:30:08.000Z"}"#
    );
    let mistakes = [
        r#"[1.5, "2026-11-14T06:00:08Z"]"#,
        r#"[-1, "2026-11-14T06:00:08Z"]"#,
        r#"[0, "2026-11-14T06:00:08+08:00"]"#,
    ];
    let messages: Vec<String> = mistakes
        .iter()
        .map(|arguments| encode_request_json(&schema, "wait _ from _", arguments).unwrap_err())
        .collect();
    assert_eq!(
        messages,
        [
            "in `duration`: found 1.5; expected a duration in whole milliseconds",
            "in `duration`: found -1; expected a duration in whole milliseconds",
            "in `start`: found \"2026-11-14T06:00:08+08:00\"; expected an instant in UTC in a string, as in \"2026-11-14T06:00:08.000Z\"",
        ]
    );
}

#[test]
fn a_field_left_out_of_json_takes_its_default() {
    let schema = common::schema(
        "PROTOCOL a VERSION 1,
        RECORD search { query: TEXT, limit: OPTIONAL: INTEGER DEFAULT 50 },
        METHOD find (search: search) RETURNS (count: INTEGER),",
    );
    let request = encode_request_json(&schema, "find _", r#"[{"query": "ode"}]"#).unwrap();
    assert_eq!(request, "find { query \"ode\", limit 50 }\n");
}

#[test]
fn a_map_is_an_object_whose_keys_stay_as_written() {
    let schema = common::schema(
        "PROTOCOL a VERSION 1,
        METHOD count (counts: MAP: INTEGER) RETURNS (waits: MAP: DURATION),",
    );
    let request =
        encode_request_json(&schema, "count _", r#"[{"hymn": 3, "old ode": 1, "": 0}]"#).unwrap();
    assert_eq!(request, "count { \"\" 0, \"hymn\" 3, \"old ode\" 1 }\n");
    assert_eq!(
        decode_request_json(&schema, &request).unwrap(),
        r#"[{"arguments":[{"":0,"hymn":3,"old ode":1}],"method":"count _"}]"#
    );
    assert_eq!(
        decode_response_json(
            &schema,
            "count _",
            "count {} RETURNS { \"tea\": 5 minutes }"
        )
        .unwrap(),
        r#"{"kind":"value","value":{"tea":300000}}"#
    );
    assert_eq!(
        encode_request_json(&schema, "count _", r#"[[3]]"#).unwrap_err(),
        "in `counts`: found [3]; expected an object"
    );
}
