//! A number's type is how it is written: `3` is an `Integer`, `3.0` a
//! `Float`.

mod common;

use anyx_protocol::{Value, decode_request};
use common::{diagnostics, schema};

const SCALE: &str = "PROTOCOL a VERSION 1,
METHOD scale (factor: Float) RETURNS (scaled: Float) ERRORS [| |],
METHOD repeat (count: Integer) RETURNS NOTHING ERRORS [| |],";

#[test]
fn a_number_with_a_point_or_an_exponent_is_a_float() {
    let schema = schema(SCALE);
    for (request, expected) in [
        ("scale 3.0", 3.0),
        ("scale 1.5", 1.5),
        ("scale -1.5", -1.5),
        ("scale 1e3", 1000.0),
        ("scale 2.5e-3", 0.0025),
    ] {
        let calls = decode_request(&schema, request).unwrap();
        assert_eq!(calls[0].arguments, [Value::Float(expected)], "{request}");
    }
    let calls = decode_request(&schema, "repeat -1").unwrap();
    assert_eq!(calls[0].arguments, [Value::Integer(-1)]);
}

#[test]
fn an_integer_does_not_fit_a_float_nor_a_float_an_integer() {
    let schema = schema(SCALE);
    let message = |request: &str| decode_request(&schema, request).unwrap_err().message;
    assert_eq!(
        message("scale 3"),
        "in `factor` of `scale _`: found the Integer `3`; expected a Float, such as `3.0`"
    );
    assert_eq!(
        message("scale -1"),
        "in `factor` of `scale _`: found the Integer `-1`; expected a Float, such as `-1.0`"
    );
    assert_eq!(
        message("repeat 1e3"),
        "in `count` of `repeat _`: found the Float `1e3`; expected an Integer, a whole number written without a point, such as `3`"
    );
}

#[test]
fn a_float_is_written_with_a_point() {
    let schema = schema(SCALE);
    let calls = vec![anyx_protocol::Call {
        method: 0,
        arguments: vec![Value::Float(3.0)],
    }];
    let request = anyx_protocol::encode_request(&schema, &calls).unwrap();
    assert_eq!(request, "scale 3.0\n");
    assert_eq!(decode_request(&schema, &request).unwrap(), calls);
}

#[test]
fn a_default_and_an_example_type_their_numbers_too() {
    let default =
        diagnostics("PROTOCOL a VERSION 1, TYPE ALIAS Row: { ratio (ratio: Float) DEFAULT ratio 1 },");
    assert!(
        default.contains("found the Integer `1`; expected a Float, such as `1.0`"),
        "{default}"
    );
    let example = diagnostics(
        "PROTOCOL a VERSION 1, METHOD scale (factor: Float) RETURNS (scaled: Float) ERRORS [| |] EXAMPLES [scale 2 RETURNS 4.0],",
    );
    assert!(
        example.contains("found the Integer `2`; expected a Float, such as `2.0`"),
        "{example}"
    );
    let returned = diagnostics(
        "PROTOCOL a VERSION 1, METHOD scale (factor: Float) RETURNS (scaled: Float) ERRORS [| |] EXAMPLES [scale 2.0 RETURNS 4],",
    );
    assert!(
        returned.contains("found the Integer `4`; expected a Float, such as `4.0`"),
        "{returned}"
    );
}

/// JSON writes `3.0` as `3`, so a number there takes its type from the
/// parameter it fills.
#[cfg(feature = "json")]
#[test]
fn json_takes_a_number_s_type_from_its_parameter() {
    use anyx_protocol::json::encode_request_json;
    let schema = schema(SCALE);
    assert_eq!(
        encode_request_json(&schema, "scale _", "[3]").unwrap(),
        "scale 3.0\n"
    );
    assert_eq!(
        encode_request_json(&schema, "repeat _", "[3]").unwrap(),
        "repeat 3\n"
    );
}
