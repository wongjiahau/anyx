//! `SCHEMA` and `SCHEMA FILES`, the calls every server answers with the
//! guide to its schema and with the schema's files.

mod common;

use anyx_protocol::server::{built_in, introspect};
use anyx_protocol::{
    Call, Outcome, Response, Value, decode_request, decode_response, encode_request,
    encode_response,
};
use common::scoped;

#[test]
fn schema_is_a_call_of_every_schema() {
    let schema = scoped();
    let introspection = schema.introspection();
    assert_eq!(introspection, schema.methods.len());
    assert_eq!(schema.method_at(introspection).full_name(), "SCHEMA");
    let calls = decode_request(&schema, "SCHEMA, list").unwrap();
    assert_eq!(
        calls,
        [
            Call {
                method: introspection,
                arguments: vec![],
            },
            Call {
                method: 0,
                arguments: vec![],
            },
        ]
    );
    assert_eq!(encode_request(&schema, &calls).unwrap(), "SCHEMA,\nlist,\n");
}

#[test]
fn schema_answers_the_guide_without_how_to_send() {
    let schema = scoped();
    let Outcome::Value(Value::Text(guide)) = introspect(&schema) else {
        panic!("expected the guide as text");
    };
    assert_eq!(guide, anyx_protocol::guide(&schema, ""));
    assert!(
        guide.starts_with("# notes (anyx protocol, version 1)\n\n## Requests\n"),
        "{guide}"
    );
    assert!(guide.contains("`SCHEMA` is a call every server answers"));
    let methods = [schema.introspection()];
    let call = Call {
        method: schema.introspection(),
        arguments: vec![],
    };
    let response = encode_response(
        &schema,
        &[call],
        &Response::Outcomes(vec![introspect(&schema)]),
    )
    .unwrap();
    assert_eq!(
        decode_response(&schema, &methods, &response).unwrap(),
        Response::Outcomes(vec![Outcome::Value(Value::Text(guide))])
    );
}

#[test]
fn schema_files_answers_the_files_the_entry_first_to_load_again() {
    let schema = scoped();
    let index = schema.schema_files();
    assert_eq!(schema.method_at(index).full_name(), "SCHEMA FILES");
    let calls = decode_request(&schema, "SCHEMA FILES").unwrap();
    assert_eq!(calls[0].method, index);
    let outcome = built_in(&schema, &calls[0]).unwrap();
    let response = encode_response(&schema, &calls, &Response::Outcomes(vec![outcome])).unwrap();
    assert!(response.starts_with("RETURNS [\n"), "{response}");
    let Response::Outcomes(outcomes) = decode_response(&schema, &[index], &response).unwrap()
    else {
        panic!("expected outcomes");
    };
    let [Outcome::Value(Value::List(files))] = outcomes.as_slice() else {
        panic!("expected a list of files");
    };
    let files: Vec<(String, String)> = files
        .iter()
        .map(|file| {
            let Value::Record(fields) = file else {
                panic!("expected a Schema File");
            };
            let text = |name: &str| match &fields[name] {
                Value::Text(text) => text.clone(),
                other => panic!("expected text, found {other:?}"),
            };
            (text("path"), text("source"))
        })
        .collect();
    assert_eq!(files[0].0, schema.files[0].path);
    assert_eq!(files.len(), schema.files.len());
    let again = anyx_protocol::Schema::load(&files[0].0, |path| {
        files
            .iter()
            .find(|(file, _)| file == path)
            .map(|(_, source)| source.clone())
            .ok_or("not answered")
    })
    .unwrap();
    assert_eq!(again, schema);
}

#[test]
fn built_in_answers_only_the_built_in_calls() {
    let schema = scoped();
    let call = decode_request(&schema, "list").unwrap().remove(0);
    assert!(built_in(&schema, &call).is_none());
}

#[test]
fn schema_is_not_in_a_scope() {
    let message = decode_request(&scoped(), "files/SCHEMA")
        .unwrap_err()
        .to_string();
    assert!(message.contains("expected one of the methods"), "{message}");
}

#[cfg(feature = "json")]
#[test]
fn the_json_client_calls_schema() {
    let schema = scoped();
    assert_eq!(
        anyx_protocol::json::encode_request_json(&schema, "SCHEMA", "[]").unwrap(),
        "SCHEMA\n"
    );
    assert_eq!(
        anyx_protocol::json::decode_response_json(&schema, "SCHEMA", "RETURNS \"guide\"").unwrap(),
        r#"{"kind":"value","value":"guide"}"#
    );
}
