//! Requests and responses, both ways.

mod common;

use std::collections::BTreeMap;

use anyx_protocol::{
    Call, Outcome, Problem, Response, Schema, Value, decode_request, decode_response,
    encode_request, encode_response,
};
use common::{assert_snapshot, poentaro, schema};

fn method(schema: &Schema, shape: &str) -> usize {
    schema
        .methods
        .iter()
        .position(|method| method.shape.text == shape)
        .unwrap_or_else(|| panic!("no method `{shape}`"))
}

fn text(text: &str) -> Value {
    Value::Text(text.to_string())
}

fn record(pairs: &[(&str, Value)]) -> Value {
    Value::Record(
        pairs
            .iter()
            .map(|(name, value)| (name.to_string(), value.clone()))
            .collect::<BTreeMap<_, _>>(),
    )
}

fn variant(shape: &str, arguments: Vec<Value>) -> Value {
    Value::Variant {
        shape: shape.to_string(),
        arguments,
    }
}

const SCORE: &str = "title \"Ode\"\n\nmelody \"\"\"\n    1 2 3 -\n    \"\"\"\n";

const WRITE: &str = "write _ at revision _ content _";

#[test]
fn a_request_reads_back_as_it_was_written() {
    let schema = poentaro();
    let calls = vec![
        Call {
            method: method(&schema, "list files"),
            arguments: vec![],
        },
        Call {
            method: method(&schema, "read _"),
            arguments: vec![text("Hymns/a.poentaro")],
        },
        Call {
            method: method(&schema, WRITE),
            arguments: vec![text("Hymns/a.poentaro"), Value::Integer(-3), text(SCORE)],
        },
    ];
    let request = encode_request(&schema, &calls).unwrap();
    assert_snapshot("tests/snapshots/request.request.anyx", &request);
    assert_eq!(decode_request(&schema, &request).unwrap(), calls);
}

#[test]
fn a_response_reads_back_as_it_was_written() {
    let schema = poentaro();
    let read = |path: &str| Call {
        method: method(&schema, "read _"),
        arguments: vec![text(path)],
    };
    let write = Call {
        method: method(&schema, WRITE),
        arguments: vec![text("Hymns/a.poentaro"), Value::Integer(3), text(SCORE)],
    };
    let calls = [
        Call {
            method: method(&schema, "list files"),
            arguments: vec![],
        },
        read("a.poentaro"),
        write.clone(),
        write,
        read("a.poentaro"),
        read("b.poentaro"),
        read("c.poentaro"),
    ];
    let methods: Vec<usize> = calls.iter().map(|call| call.method).collect();
    let summary = |path: &str, revision| {
        record(&[("path", text(path)), ("revision", Value::Integer(revision))])
    };
    let diagnostic = record(&[
        ("line", Value::Integer(2)),
        ("column", Value::Integer(5)),
        ("code", text("E1")),
        ("message", text("found \"x\"; expected a note")),
    ]);
    let response = Response::Outcomes(vec![
        Outcome::Value(Value::List(vec![
            summary("a.poentaro", 1),
            summary("b/c.poentaro", 7),
        ])),
        Outcome::Value(record(&[
            ("path", text("a.poentaro")),
            ("revision", Value::Integer(1)),
            ("content", text(SCORE)),
        ])),
        Outcome::Error(variant(
            "conflict at revision _ content _",
            vec![Value::Integer(4), text(SCORE)],
        )),
        Outcome::Error(variant("rejected _", vec![Value::List(vec![diagnostic])])),
        Outcome::Error(variant("trashed", vec![])),
        Outcome::InternalError,
        Outcome::NotRun,
    ]);
    let text = encode_response(&schema, &calls, &response).unwrap();
    assert_snapshot("tests/snapshots/response.response.anyx", &text);
    assert_eq!(decode_response(&schema, &methods, &text).unwrap(), response);
}

#[test]
fn an_error_must_be_one_of_the_methods() {
    let schema = poentaro();
    let response = Response::Outcomes(vec![Outcome::Error(variant(
        "invalid path _",
        vec![text("x")],
    ))]);
    let read = Call {
        method: method(&schema, "read _"),
        arguments: vec![text("a.poentaro")],
    };
    let error = encode_response(&schema, &[read], &response).unwrap_err();
    assert_eq!(
        error.message,
        "found the error `invalid path _`; expected an error of `read _`"
    );
}

#[test]
fn an_invalid_request_reads_back() {
    let schema = poentaro();
    let response = Response::InvalidRequest(Problem {
        line: 2,
        column: 4,
        message: "found `x`; expected \"y\"".to_string(),
    });
    let text = encode_response(&schema, &[], &response).unwrap();
    assert_eq!(
        text,
        "INVALID REQUEST { line 2, column 4, message \"found `x`; expected \\\"y\\\"\" }\n"
    );
    assert_eq!(decode_response(&schema, &[0, 1], &text).unwrap(), response);
}

const MODES: &str = "PROTOCOL a VERSION 1,
TYPE ALIAS Shade: UNION [`dark, `light],
TYPE ALIAS Mode: UNION [`dark, `light, `custom hue (hue: Float), `(base: Shade) dimmed],
RECORD Options { mode: Mode, strict (Boolean) DEFAULT strict (`false) },
METHOD set (mode: Mode) strict (strict: Boolean) RETURNS (ok: Boolean) ERRORS [],
METHOD apply (options: Options) RETURNS (modes: LIST: Mode) ERRORS [],";

#[test]
fn words_in_slots_go_in_parentheses() {
    let schema = schema(MODES);
    let custom = variant("custom hue _", vec![Value::Float(0.5)]);
    let calls = vec![
        Call {
            method: 0,
            arguments: vec![
                variant("dark", vec![]),
                Value::Variant {
                    shape: "true".into(),
                    arguments: vec![],
                },
            ],
        },
        Call {
            method: 0,
            arguments: vec![
                custom.clone(),
                Value::Variant {
                    shape: "false".into(),
                    arguments: vec![],
                },
            ],
        },
        Call {
            method: 0,
            arguments: vec![
                variant("_ dimmed", vec![variant("dark", vec![])]),
                Value::Variant {
                    shape: "false".into(),
                    arguments: vec![],
                },
            ],
        },
        Call {
            method: 1,
            arguments: vec![record(&[
                ("mode", variant("_ dimmed", vec![variant("light", vec![])])),
                ("strict", variant("false", vec![])),
            ])],
        },
    ];
    let request = encode_request(&schema, &calls).unwrap();
    assert_eq!(
        request,
        "set (`dark AS Mode) strict (`true),
set (`custom hue 0.5) strict (`false),
set (`(`dark) dimmed) strict (`false),
apply { mode: `(`light) dimmed, strict: `false },
"
    );
    assert_eq!(decode_request(&schema, &request).unwrap(), calls);
}

#[test]
fn text_that_a_raw_string_cannot_hold_is_quoted() {
    let schema = poentaro();
    for content in ["a\n   \nb", "\"\"\"\nx", "x\r\ny", "tab\there\n"] {
        let calls = vec![Call {
            method: method(&schema, "create _ content _"),
            arguments: vec![text("a"), text(content)],
        }];
        let request = encode_request(&schema, &calls).unwrap();
        assert_eq!(
            decode_request(&schema, &request).unwrap(),
            calls,
            "{request}"
        );
    }
}

#[test]
fn a_number_that_is_not_finite_has_no_form() {
    let schema = schema("PROTOCOL a VERSION 1, METHOD f (x: Float) RETURNS (y: Float) ERRORS [],");
    let calls = vec![Call {
        method: 0,
        arguments: vec![Value::Float(f64::NAN)],
    }];
    assert!(encode_request(&schema, &calls).is_err());
}

fn render(cases: &[&str], decode: impl Fn(&str) -> Result<String, Problem>) -> String {
    cases
        .iter()
        .map(|source| match decode(source) {
            Ok(read) => panic!("{source:?} read as {read}"),
            Err(problem) => format!("{source}\n=> {problem}\n"),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn mistakes_in_a_request_point_at_where_they_are() {
    let schema = poentaro();
    let cases = [
        "",
        "reed \"a\"",
        "read 3",
        "write \"a\" at revision 1.5 content \"x\"",
        "write \"a\" at revision 99999999999999999999 content \"x\"",
        "read \"a\",\n\"loose\"",
        "read \"a",
        "list files, read [\"a\"]",
    ];
    assert_snapshot(
        "tests/snapshots/request_errors.txt",
        &render(&cases, |source| {
            decode_request(&schema, source).map(|calls| format!("{calls:?}"))
        }),
    );
}

#[test]
fn mistakes_in_values_name_the_parameter_and_field() {
    let schema = schema(MODES);
    let cases = [
        "apply { mode: `dim }",
        "apply { strict: `true }",
        "apply { mode: `dark, mode: `dark }",
        "apply { mode: `dark, colour: `dark }",
        "apply { mode: `custom hue }",
        "apply { mode: `dark hue 1 }",
        "apply { mode: `custom hue \"red\" }",
        "apply { mode `dark }",
        "apply { mode: `dark, strict: `yes }",
        "set dark strict (`true)",
    ];
    assert_snapshot(
        "tests/snapshots/value_errors.txt",
        &render(&cases, |source| {
            decode_request(&schema, source).map(|calls| format!("{calls:?}"))
        }),
    );
}

#[test]
fn mistakes_in_a_response_point_at_where_they_are() {
    let schema = poentaro();
    let read = [method(&schema, "read _")];
    let cases = [
        "ERROR `conflict at revision 1 content \"x\"",
        "ERROR `gone",
        "RETURNS missing",
        "NOT RUN, NOT RUN",
        "",
        "INVALID REQUEST { line 1 }",
        "missing",
        "read \"a\" RETURNS { path \"a\", revision 1, content \"x\" }",
        "ERRORS missing",
    ];
    assert_snapshot(
        "tests/snapshots/response_errors.txt",
        &render(&cases, |source| {
            decode_response(&schema, &read, source).map(|response| format!("{response:?}"))
        }),
    );
}

const ACCOUNTS: &str = "PROTOCOL a VERSION 1,
METHOD change password from (old: Text) to (new: Text) RETURNS NOTHING ERRORS [`wrong password],
METHOD tag (names: LIST Text) RETURNS (count: Integer) ERRORS [],";

#[test]
fn a_response_answers_each_call_by_position_without_repeating_it() {
    let schema = schema(ACCOUNTS);
    let change_password = Call {
        method: 0,
        arguments: vec![text("old secret"), text("new secret")],
    };
    let tag = |names: &[&str]| Call {
        method: 1,
        arguments: vec![Value::List(names.iter().map(|name| text(name)).collect())],
    };
    let calls = [
        change_password.clone(),
        change_password,
        tag(&["n1", "n2"]),
        tag(&[]),
        tag(&["n3"]),
    ];
    let response = Response::Outcomes(vec![
        Outcome::Value(Value::Nothing),
        Outcome::Error(variant("wrong password", vec![])),
        Outcome::Value(Value::Integer(2)),
        Outcome::InternalError,
        Outcome::NotRun,
    ]);
    let written = encode_response(&schema, &calls, &response).unwrap();
    assert_eq!(
        written,
        "RETURNS NOTHING,\nERROR `wrong password,\nRETURNS 2,\nINTERNAL ERROR,\nNOT RUN,\n"
    );
    assert!(!written.contains("secret"));
    assert_eq!(
        decode_response(&schema, &[0, 0, 1, 1, 1], &written).unwrap(),
        response
    );
}

#[test]
fn a_result_is_read_as_an_outcome_of_the_call_in_its_place() {
    let schema = schema(ACCOUNTS);
    let cases = [
        "RETURNS 2, RETURNS NOTHING",
        "RETURNS NOTHING, RETURNS NOTHING",
        "RETURNS NOTHING",
        "RETURNS NOTHING, RETURNS 2, RETURNS 3",
        "RETURNS NOTHING, ERROR `wrong password",
        "change password from \"a\" to \"b\" RETURNS NOTHING, tag [] RETURNS 0",
    ];
    assert_snapshot(
        "tests/snapshots/positional_response_errors.txt",
        &render(&cases, |source| {
            decode_response(&schema, &[0, 1], source).map(|response| format!("{response:?}"))
        }),
    );
}

#[test]
fn a_method_that_returns_nothing_answers_returns_nothing() {
    let schema = schema(
        "PROTOCOL a VERSION 1,
        METHOD sign out RETURNS NOTHING ERRORS [],
        METHOD count RETURNS (count: Integer) ERRORS [],",
    );
    let sign_out = method(&schema, "sign out");
    let count = method(&schema, "count");
    let calls = [
        Call {
            method: sign_out,
            arguments: vec![],
        },
        Call {
            method: count,
            arguments: vec![],
        },
    ];
    let response = Response::Outcomes(vec![
        Outcome::Value(Value::Nothing),
        Outcome::Value(Value::Integer(2)),
    ]);
    let written = encode_response(&schema, &calls, &response).unwrap();
    assert_eq!(written, "RETURNS NOTHING,\nRETURNS 2,\n");
    assert_eq!(
        decode_response(&schema, &[sign_out, count], &written).unwrap(),
        response
    );
}

#[test]
fn nothing_and_a_value_are_not_mixed_up() {
    let schema = schema(
        "PROTOCOL a VERSION 1,
        METHOD sign out RETURNS NOTHING ERRORS [],
        METHOD count RETURNS (count: Integer) ERRORS [],",
    );
    let sign_out = method(&schema, "sign out");
    let count = method(&schema, "count");
    assert!(
        decode_response(&schema, &[sign_out], "RETURNS 2")
            .unwrap_err()
            .message
            .contains("expected `RETURNS NOTHING`")
    );
    assert!(
        decode_response(&schema, &[count], "RETURNS NOTHING")
            .unwrap_err()
            .message
            .contains("found `RETURNS NOTHING`")
    );
    let call = Call {
        method: sign_out,
        arguments: vec![],
    };
    assert!(
        encode_response(
            &schema,
            &[call],
            &Response::Outcomes(vec![Outcome::Value(Value::Integer(1))])
        )
        .is_err()
    );
}

const TIMERS: &str = "PROTOCOL a VERSION 1,
RECORD Timer { at: Instant, every: Duration, pause (Duration) DEFAULT pause (0 seconds) },
METHOD wait (duration: Duration) until (instant: Instant) RETURNS (started: Instant) ERRORS [],
METHOD set (timer: Timer) RETURNS (left: Duration) ERRORS [],
METHOD list RETURNS (durations: LIST Duration) ERRORS [],
METHOD snooze (duration: Duration?) RETURNS (left: Duration) ERRORS [],";

fn instant(text: &str) -> Value {
    let milliseconds = |at: &str| -> u64 {
        // Only the instants below: 2026-11-14 and the epoch.
        match at {
            "2026-11-14T06:00:08.000Z" => 1_794_636_008_000,
            "1970-01-01T00:00:00.250Z" => 250,
            _ => panic!("{at}"),
        }
    };
    Value::Instant(std::time::UNIX_EPOCH + std::time::Duration::from_millis(milliseconds(text)))
}

fn duration(milliseconds: u64) -> Value {
    Value::Duration(std::time::Duration::from_millis(milliseconds))
}

#[test]
fn instants_and_durations_read_back_as_written() {
    let schema = schema(TIMERS);
    let calls = vec![
        Call {
            method: 0,
            arguments: vec![duration(5 * 60_000), instant("2026-11-14T06:00:08.000Z")],
        },
        Call {
            method: 1,
            arguments: vec![record(&[
                ("at", instant("1970-01-01T00:00:00.250Z")),
                ("every", duration(5_400_000)),
                ("pause", duration(0)),
            ])],
        },
    ];
    let request = encode_request(&schema, &calls).unwrap();
    assert_eq!(
        request,
        "wait (5 minutes) until (\"2026-11-14T06:00:08.000Z\" AS Instant),
set { at \"1970-01-01T00:00:00.250Z\", every: 1 hours 30 minutes, pause: 0 seconds },
"
    );
    assert_eq!(decode_request(&schema, &request).unwrap(), calls);
    let response = Response::Outcomes(vec![
        Outcome::Value(instant("2026-11-14T06:00:08.000Z")),
        Outcome::Value(duration(90_001)),
    ]);
    let written = encode_response(&schema, &calls, &response).unwrap();
    assert_eq!(
        written,
        "RETURNS \"2026-11-14T06:00:08.000Z\",\nRETURNS 1 minutes 30 seconds 1 milliseconds,\n"
    );
    assert_eq!(
        decode_response(&schema, &[0, 1], &written).unwrap(),
        response
    );
}

#[test]
fn an_instant_and_a_duration_read_in_other_forms_too() {
    let schema = schema(TIMERS);
    let calls = decode_request(
        &schema,
        "wait (90 seconds) until (\"2026-11-14T06:00:08Z\" AS Instant)",
    )
    .unwrap();
    assert_eq!(
        calls[0].arguments,
        [duration(90_000), instant("2026-11-14T06:00:08.000Z")]
    );
    let list = decode_response(
        &schema,
        &[2],
        "RETURNS [1 days, 2 hours 1 minutes, 1 seconds 5 milliseconds]",
    )
    .unwrap();
    assert_eq!(
        list,
        Response::Outcomes(vec![Outcome::Value(Value::List(vec![
            duration(86_400_000),
            duration(7_260_000),
            duration(1_005),
        ]))])
    );
}

#[test]
fn mistakes_in_instants_and_durations_say_how_to_write_them() {
    let schema = schema(TIMERS);
    let cases = [
        "wait (5) until \"2026-11-14T06:00:08Z\"",
        "wait (5 weeks) until \"2026-11-14T06:00:08Z\"",
        "wait (1 minutes 1 hours) until \"2026-11-14T06:00:08Z\"",
        "wait (1.5 hours) until \"2026-11-14T06:00:08Z\"",
        "wait (1 hour 30 minutes) until \"2026-11-14T06:00:08Z\"",
        "wait (-1 hours) until \"2026-11-14T06:00:08Z\"",
        "wait (minutes) until \"2026-11-14T06:00:08Z\"",
        "snooze (1 hour)",
        "wait (5 minutes) until \"2026-11-14 06:00:08\"",
        "wait (5 minutes) until \"2026-11-14T06:00:08+08:00\"",
        "wait (5 minutes) until 1794636008",
        "set { at \"2026-11-14T06:00:08Z\", every 5 minutes }",
    ];
    assert_snapshot(
        "tests/snapshots/time_errors.txt",
        &render(&cases, |source| {
            decode_request(&schema, source).map(|calls| format!("{calls:?}"))
        }),
    );
}

#[test]
fn a_field_left_out_takes_its_default() {
    let schema = schema(
        "PROTOCOL a VERSION 1,
        RECORD Search { query: Text, limit (Integer) DEFAULT limit 50, note (Text) DEFAULT note \"\" },
        METHOD find (search: Search) RETURNS (search: Search) ERRORS [],",
    );
    let calls = decode_request(&schema, "find { query \"ode\" }").unwrap();
    let filled = record(&[
        ("query", text("ode")),
        ("limit", Value::Integer(50)),
        ("note", text("")),
        ("note", text("")),
    ]);
    assert_eq!(calls[0].arguments, std::slice::from_ref(&filled));
    assert_eq!(
        encode_request(&schema, &calls).unwrap(),
        "find { query \"ode\", limit 50, note \"\" }\n"
    );
    let response = decode_response(&schema, &[0], "RETURNS { query \"a\", limit 3 }").unwrap();
    assert_eq!(
        response,
        Response::Outcomes(vec![Outcome::Value(record(&[
            ("query", text("a")),
            ("limit", Value::Integer(3)),
            ("note", text(""))
        ]))])
    );
}

const COUNTS: &str = "PROTOCOL a VERSION 1,
TYPE ALIAS Mood: UNION [`calm, `wild],
RECORD Tally { counts: MAP Integer, moods (MAP: Mood) DEFAULT moods {} },
METHOD count (tally: Tally) RETURNS (counts: MAP Integer) ERRORS [],
METHOD wait (waits: MAP Duration) RETURNS (moods: MAP: LIST: Mood) ERRORS [],";

fn map(entries: &[(&str, Value)]) -> Value {
    Value::Map(
        entries
            .iter()
            .map(|(key, value)| (key.to_string(), value.clone()))
            .collect(),
    )
}

#[test]
fn maps_read_back_as_written() {
    let schema = schema(COUNTS);
    let calls = vec![
        Call {
            method: 0,
            arguments: vec![record(&[
                (
                    "counts",
                    map(&[("hymn", Value::Integer(3)), ("ode", Value::Integer(1))]),
                ),
                (
                    "moods",
                    map(&[
                        ("a \"b\"", variant("calm", vec![])),
                        ("", variant("wild", vec![])),
                    ]),
                ),
            ])],
        },
        Call {
            method: 1,
            arguments: vec![map(&[("tea", duration(300_000))])],
        },
        Call {
            method: 1,
            arguments: vec![map(&[])],
        },
    ];
    let request = encode_request(&schema, &calls).unwrap();
    assert_eq!(
        request,
        "count { counts { \"hymn\" 3, \"ode\" 1 }, moods { \"\": `wild, \"a \\\"b\\\"\": `calm } },
wait { \"tea\": 5 minutes },
wait ({} AS: MAP Duration),
"
    );
    assert_eq!(decode_request(&schema, &request).unwrap(), calls);
    let response = Response::Outcomes(vec![
        Outcome::Value(map(&[("hymn", Value::Integer(3))])),
        Outcome::Value(map(&[(
            "tea",
            Value::List(vec![variant("calm", vec![]), variant("wild", vec![])]),
        )])),
        Outcome::Value(map(&[])),
    ]);
    let written = encode_response(&schema, &calls, &response).unwrap();
    assert_eq!(
        decode_response(&schema, &[0, 1, 1], &written).unwrap(),
        response
    );
}

#[test]
fn a_map_field_left_out_takes_its_default() {
    let schema = schema(COUNTS);
    let calls = decode_request(&schema, "count { counts { \"hymn\": 3 } }").unwrap();
    assert_eq!(
        calls[0].arguments,
        [record(&[
            ("counts", map(&[("hymn", Value::Integer(3))])),
            ("moods", map(&[])),
        ])]
    );
}

#[test]
fn mistakes_in_maps_say_how_to_write_them() {
    let schema = schema(COUNTS);
    let cases = [
        "count { counts [1, 2] }",
        "count { counts { hymn 3 } }",
        "count { counts { \"hymn\" 3, \"hymn\" 4 } }",
        "count { counts { \"hymn\" \"three\" } }",
        "count { counts { \"hymn\" } }",
        "wait { \"tea\" 5 minutes }",
    ];
    assert_snapshot(
        "tests/snapshots/map_errors.txt",
        &render(&cases, |source| {
            decode_request(&schema, source).map(|calls| format!("{calls:?}"))
        }),
    );
}
