use anyx_protocol::{Call, Outcome, Schema, Value};
use anyx_shell::{Interpreter, Tool, environment::Environment};

fn interpreter() -> Interpreter {
    let mut interpreter = Interpreter::new();
    interpreter
        .register("environment", Environment::new())
        .unwrap();
    interpreter
}

fn run(interpreter: &mut Interpreter, source: &str) -> Vec<Value> {
    let execution = interpreter.execute(source).unwrap();
    assert!(execution.error.is_none(), "{:?}", execution.error);
    let parsed = anyx::parse(&execution.output);
    assert!(parsed.errors.is_empty(), "{}", execution.output);
    assert_eq!(parsed.syntax.nodes().len(), execution.values.len());
    execution.values
}

#[test]
fn example_returns_values_without_response_envelopes() {
    let mut interpreter = interpreter();
    let execution = interpreter
        .execute(include_str!("../examples/environment.shell.anyx"))
        .unwrap();
    assert!(execution.error.is_none());
    assert_eq!(
        execution.output,
        "{ name \"ANYX_EXAMPLE\", value \"hello\" },\n\"hello\",\n`true,\n`false,\n"
    );
    assert_eq!(execution.values.len(), 4);
}

#[test]
fn listing_is_sorted_and_empty_is_distinct_from_absent() {
    let mut interpreter = interpreter();
    run(
        &mut interpreter,
        "environment / set variable \"z\" to \"last\", environment / set variable \"a\" to \"\"",
    );
    let values = run(
        &mut interpreter,
        "environment / list variables, environment / read variable \"a\"",
    );
    let Value::List(variables) = &values[0] else {
        panic!()
    };
    let Value::Record(first) = &variables[0] else {
        panic!()
    };
    assert_eq!(first["name"], Value::Text("a".into()));
    assert_eq!(values[1], Value::Text(String::new()));
    assert!(
        interpreter
            .execute("environment / read variable \"absent\"")
            .unwrap()
            .error
            .is_some()
    );
}

#[test]
fn complete_document_is_checked_before_any_side_effects() {
    let mut interpreter = interpreter();
    for invalid in [
        "environment / set variable \"x\" to \"changed\", unknown / list variables",
        "environment / set variable \"x\" to \"changed\", environment / read variable 1",
        "environment / set variable \"x\" to \"changed\", environment / unknown method",
        "environment / set variable \"x\" to \"changed\", environment / read variable (",
        "environment / set variable \"x\" to \"changed\", list variables",
    ] {
        assert!(interpreter.execute(invalid).is_err(), "{invalid}");
        assert_eq!(
            run(&mut interpreter, "environment / list variables"),
            vec![Value::List(vec![])]
        );
    }
}

#[test]
fn declared_error_stops_execution_and_keeps_earlier_effects() {
    let mut interpreter = interpreter();
    let result = interpreter.execute("environment / set variable \"kept\" to \"yes\", environment / read variable \"missing\", environment / remove variable \"kept\"").unwrap();
    assert_eq!(result.values.len(), 1);
    assert_eq!(
        result.error.as_deref(),
        Some("ERROR `variable does not exist\n")
    );
    assert_eq!(
        run(&mut interpreter, "environment / read variable \"kept\""),
        vec![Value::Text("yes".into())]
    );
}

#[test]
fn invalid_names_and_values_do_not_mutate_the_environment() {
    let mut interpreter = interpreter();
    for source in [
        "environment / set variable \"\" to \"value\"",
        "environment / read variable \"a=b\"",
        "environment / remove variable \"a\\u{0}b\"",
        "environment / set variable \"x\" to \"a\\u{0}b\"",
    ] {
        assert!(
            interpreter.execute(source).unwrap().error.is_some(),
            "{source}"
        );
    }
    assert_eq!(
        run(&mut interpreter, "environment / list variables"),
        vec![Value::List(vec![])]
    );
}

#[test]
fn unicode_multiline_and_escaped_values_round_trip() {
    let mut interpreter = interpreter();
    let execution = interpreter.execute(r#"environment / set variable "问候" to "hello\n世界\t\"\\", environment / read variable "问候""#).unwrap();
    assert!(execution.error.is_none());
    assert_eq!(execution.values[1], Value::Text("hello\n世界\t\"\\".into()));
    let parsed = anyx::parse(&execution.output);
    assert!(parsed.errors.is_empty());
    let anyx::NodeKind::String(value) = &parsed.syntax.nodes()[1].kind else {
        panic!()
    };
    assert_eq!(value.value, "hello\n世界\t\"\\");
}

#[test]
fn validation_reports_positions_in_the_whole_document() {
    let mut interpreter = interpreter();
    let source = "environment / list variables,\nenvironment / read variable 3";
    let problem = interpreter.execute(source).unwrap_err();
    assert_eq!((problem.line, problem.column), (2, 29));
    let problem = interpreter
        .execute("environment / read variable\n  3")
        .unwrap_err();
    assert_eq!((problem.line, problem.column), (2, 3));
}

#[test]
fn schema_introspection_is_scoped_and_returns_a_text_value() {
    let mut interpreter = interpreter();
    let values = run(&mut interpreter, "environment / SCHEMA");
    let Value::Text(guide) = &values[0] else {
        panic!()
    };
    assert!(guide.contains("set variable"));
    assert!(interpreter.execute("SCHEMA").is_err());
}

#[test]
fn independent_tools_can_expose_the_same_method_names() {
    let mut interpreter = interpreter();
    interpreter.register("second", Environment::new()).unwrap();
    run(
        &mut interpreter,
        "environment / set variable \"x\" to \"first\", second / set variable \"x\" to \"second\"",
    );
    assert_eq!(
        run(
            &mut interpreter,
            "environment/read variable \"x\", second / read variable \"x\""
        ),
        vec![Value::Text("first".into()), Value::Text("second".into())]
    );
    assert!(
        interpreter
            .register("environment", Environment::new())
            .is_err()
    );
    assert!(
        interpreter
            .register("two words", Environment::new())
            .is_err()
    );
}

#[test]
fn empty_or_comment_only_documents_do_nothing() {
    let mut interpreter = interpreter();
    assert!(run(&mut interpreter, "").is_empty());
    assert!(run(&mut interpreter, "## comment\n").is_empty());
}

struct BrokenTool(Schema);
impl Tool for BrokenTool {
    fn schema(&self) -> &Schema {
        &self.0
    }
    fn invoke(&mut self, _: &Call) -> Outcome {
        Outcome::Value(Value::Text("wrong type".into()))
    }
}

#[test]
fn invalid_tool_output_is_an_internal_failure() {
    let mut interpreter = interpreter();
    interpreter
        .register(
            "broken",
            BrokenTool(
                Schema::parse(
                    "PROTOCOL broken VERSION 1, METHOD count RETURNS (count ⁽Integer) ERRORS [| |]",
                )
                .unwrap(),
            ),
        )
        .unwrap();
    let execution = interpreter
        .execute("broken / count, environment / set variable \"x\" to \"no\"")
        .unwrap();
    assert_eq!(execution.error.as_deref(), Some("INTERNAL ERROR\n"));
    assert!(execution.output.is_empty());
    assert_eq!(
        run(&mut interpreter, "environment / list variables"),
        vec![Value::List(vec![])]
    );
}

struct ExampleTool(Schema);
impl Tool for ExampleTool {
    fn schema(&self) -> &Schema {
        &self.0
    }
    fn invoke(&mut self, call: &Call) -> Outcome {
        Outcome::Value(call.arguments.first().cloned().unwrap_or(Value::Nothing))
    }
}

#[test]
fn tool_bodies_keep_protocol_records_lists_and_nothing_returns() {
    let mut interpreter = interpreter();
    interpreter.register("example", ExampleTool(Schema::parse(
        "PROTOCOL example VERSION 1, TYPE ALIAS Entry ⁽{ names (names ⁽LIST Text) }, METHOD echo (entry ⁽Entry) RETURNS (entry ⁽Entry) ERRORS [| |], METHOD finish RETURNS NOTHING ERRORS [| |]"
    ).unwrap())).unwrap();
    let execution = interpreter.execute(
        "## a shell comment\nexample / echo { names ⁽[\"a/b\", \"c,d\"] },\nexample / finish,\nenvironment / list variables,"
    ).unwrap();
    assert!(execution.error.is_none());
    assert_eq!(execution.values.len(), 2);
    assert_eq!(execution.output, "{ names [\"a/b\", \"c,d\"] },\n[],\n");
}

#[test]
fn preconditions_are_checked_before_a_call_runs() {
    struct SettingsTool(Schema);
    impl Tool for SettingsTool {
        fn schema(&self) -> &Schema {
            &self.0
        }
        fn invoke(&mut self, call: &Call) -> Outcome {
            let setting = |shape: &str| Value::Variant {
                shape: shape.into(),
                arguments: vec![],
            };
            Outcome::Value(
                call.arguments.first().cloned().unwrap_or_else(|| {
                    Value::List(vec![setting("any case"), setting("exact case")])
                }),
            )
        }
    }
    let mut interpreter = interpreter();
    interpreter
        .register(
            "example",
            SettingsTool(
                Schema::parse(
                    r#"PROTOCOL example VERSION 1,
                    TYPE ALIAS Setting [| `any case, `exact case, `hidden |],
                    METHOD echo (settings ⁽LIST Setting)
                        REQUIRES [settings₎ has at most one of [`any case, `exact case]]
                        RETURNS (settings ⁽LIST Setting)
                        ERRORS [| |],
                    METHOD both RETURNS (settings ⁽LIST Setting) ERRORS [| |],"#,
                )
                .unwrap(),
            ),
        )
        .unwrap();
    // Written out, the arguments are checked with the rest of the document,
    // so nothing runs.
    let problem = interpreter
        .execute(
            "environment / set variable \"x\" to \"1\",\nexample / echo [`any case, `exact case]",
        )
        .unwrap_err();
    assert_eq!(problem.line, 2);
    assert!(
        problem
            .message
            .contains("expected settings has at most one of"),
        "{}",
        problem.message
    );
    assert_eq!(
        run(&mut interpreter, "environment / list variables"),
        vec![Value::List(vec![])]
    );
    // Computed, they are checked just before the call.
    let execution = interpreter
        .execute("example / echo [`hidden], example / echo (example / both)")
        .unwrap();
    assert_eq!(execution.values.len(), 1);
    let error = execution.error.unwrap();
    assert!(error.starts_with("INVALID REQUEST"), "{error}");
    assert!(
        error.contains("expected settings has at most one of"),
        "{error}"
    );
}
