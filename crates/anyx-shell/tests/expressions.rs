use std::cell::RefCell;
use std::rc::Rc;

use anyx_protocol::{Call, Outcome, Schema, Value};
use anyx_shell::{Interpreter, Tool, environment::Environment};

struct Example {
    schema: Schema,
    calls: Rc<RefCell<Vec<String>>>,
}

fn interpreter() -> (Interpreter, Rc<RefCell<Vec<String>>>) {
    let mut interpreter = Interpreter::new();
    interpreter
        .register("environment", Environment::new())
        .unwrap();
    let calls = Rc::new(RefCell::new(Vec::new()));
    interpreter
        .register(
            "example",
            Example {
                schema: Schema::parse(
                    "PROTOCOL example VERSION 1,
            RECORD Envelope { items: LIST Text },
            CHOICE Message { `message (text: Text) },
            METHOD text RETURNS (value: Text),
            METHOD integer RETURNS (value: Integer),
            METHOD names RETURNS (names: LIST Text),
            METHOD handle (value: Text) RETURNS (handled: Text),
            METHOD handle (value: Integer) RETURNS (handled: Text),
            METHOD handle (value: LIST Text) RETURNS (handled: Text),
            METHOD handle (value: LIST Integer) RETURNS (handled: Text),
            METHOD envelope (value: Envelope) RETURNS (value: Envelope),
            METHOD mapping (value: MAP Text) RETURNS (value: MAP Text),
            METHOD message (value: Message) RETURNS (value: Message),
            METHOD pair (left: Text) with (right: Text) RETURNS (pair: LIST Text),
            METHOD finish RETURNS NOTHING",
                )
                .unwrap(),
                calls: calls.clone(),
            },
        )
        .unwrap();
    (interpreter, calls)
}

impl Tool for Example {
    fn schema(&self) -> &Schema {
        &self.schema
    }
    fn invoke(&mut self, call: &Call) -> Outcome {
        let method = self.schema.method_at(call.method);
        self.calls
            .borrow_mut()
            .push(self.schema.typed_shape(&method.shape));
        Outcome::Value(match method.shape.text.as_str() {
            "text" => Value::Text("hello".into()),
            "integer" => Value::Integer(42),
            "names" => Value::List(vec![]),
            "finish" => Value::Nothing,
            "handle _" => Value::Text(self.schema.type_name(&method.shape.parameters[0].r#type)),
            "pair _ with _" => Value::List(call.arguments.clone()),
            _ => call.arguments[0].clone(),
        })
    }
}

#[test]
fn nested_environment_call_supplies_a_value_without_printing_it() {
    let (mut interpreter, _) = interpreter();
    let execution = interpreter.execute("environment / set variable \"HOME\" to \"/home/example\", environment / set variable \"COPY\" to (environment / read variable \"HOME\")").unwrap();
    assert!(execution.error.is_none(), "{:?}", execution.error);
    assert_eq!(execution.values.len(), 2);
    let execution = interpreter
        .execute("environment / read variable \"COPY\"")
        .unwrap();
    assert_eq!(execution.values, vec![Value::Text("/home/example".into())]);
    assert_eq!(execution.output, "\"/home/example\"\n");
}

#[test]
fn declared_return_types_disambiguate_overloads_even_for_empty_lists() {
    let (mut interpreter, calls) = interpreter();
    let execution = interpreter.execute("example / handle (example / text), example / handle (example / integer), example / handle (example / names)").unwrap();
    assert!(execution.error.is_none());
    assert_eq!(
        execution.values,
        vec![
            Value::Text("Text".into()),
            Value::Text("Integer".into()),
            Value::Text("LIST Text".into())
        ]
    );
    assert_eq!(
        *calls.borrow(),
        [
            "text",
            "handle (value: Text)",
            "integer",
            "handle (value: Integer)",
            "names",
            "handle (value: LIST Text)"
        ]
    );
}

#[test]
fn argument_types_are_checked_before_any_calls_run() {
    let (mut interpreter, calls) = interpreter();
    for source in [
        "example / text, environment / read variable (example / integer)",
        "example / text, environment / read variable (missing / text)",
        "example / text, environment / read variable (example / finish)",
        "example / text, environment / read variable (example / unknown)",
        "example / text, environment / read variable ((example / integer) AS: Text)",
        "example / text, environment / read variable (example / pair (example / text) with (example / text))",
    ] {
        assert!(interpreter.execute(source).is_err(), "{source}");
        assert!(calls.borrow().is_empty(), "{source}");
    }
}

#[test]
fn arguments_run_left_to_right_before_the_parent() {
    let (mut interpreter, calls) = interpreter();
    let execution = interpreter
        .execute("example / pair (example / text) with (example / handle (example / integer))")
        .unwrap();
    assert!(execution.error.is_none());
    assert_eq!(
        *calls.borrow(),
        [
            "text",
            "integer",
            "handle (value: Integer)",
            "pair (left: Text) with (right: Text)"
        ]
    );
    assert_eq!(
        execution.values,
        vec![Value::List(vec![
            Value::Text("hello".into()),
            Value::Text("Integer".into())
        ])]
    );
}

#[test]
fn nested_failure_skips_remaining_arguments_and_parent() {
    let (mut interpreter, calls) = interpreter();
    let execution = interpreter
        .execute("example / pair (environment / read variable \"missing\") with (example / text)")
        .unwrap();
    assert_eq!(
        execution.error.as_deref(),
        Some("ERROR `variable does not exist\n")
    );
    assert!(execution.values.is_empty());
    assert!(calls.borrow().is_empty());
}

#[test]
fn expressions_work_in_lists_records_maps_and_choice_arguments() {
    let (mut interpreter, _) = interpreter();
    for source in [
        "example / handle [(example / text)]",
        "example / envelope { items: [(example / text), (example / text)] }",
        "example / mapping { \"key\": (example / text) }",
        "example / message (`message (example / text))",
        "example / handle ((example / names) AS: LIST Text)",
    ] {
        let execution = interpreter
            .execute(source)
            .unwrap_or_else(|error| panic!("{source}: {error}"));
        assert!(execution.error.is_none(), "{source}: {:?}", execution.error);
        assert_eq!(execution.values.len(), 1);
        assert!(anyx::parse(&execution.output).errors.is_empty());
    }
}

#[test]
fn anyx_sugar_groups_argument_expressions() {
    let (mut interpreter, _) = interpreter();
    let execution = interpreter
        .execute("example / handle: example / text")
        .unwrap();
    assert_eq!(execution.values, vec![Value::Text("Text".into())]);
    let execution = interpreter
        .execute("example / message: `message: example / text")
        .unwrap();
    assert!(execution.error.is_none());
    assert_eq!(
        execution.values,
        vec![Value::Variant {
            shape: "message _".into(),
            arguments: vec![Value::Text("hello".into())]
        }]
    );
}

struct Identity(Schema);
impl Tool for Identity {
    fn schema(&self) -> &Schema {
        &self.0
    }
    fn invoke(&mut self, call: &Call) -> Outcome {
        Outcome::Value(call.arguments[0].clone())
    }
}

#[test]
fn named_result_types_keep_their_identity_across_protocols() {
    let (mut interpreter, _) = interpreter();
    interpreter.register("source", Identity(Schema::parse(
        "PROTOCOL source VERSION 1, NAMED TYPE Identifier (Text), METHOD identity (value: Identifier) RETURNS (value: Identifier)"
    ).unwrap())).unwrap();
    interpreter.register("target", Identity(Schema::parse(
        "PROTOCOL target VERSION 1, RECORD Earlier { count: Integer }, NAMED TYPE Identifier (Text), METHOD identity (value: Identifier) RETURNS (value: Identifier)"
    ).unwrap())).unwrap();
    let execution = interpreter
        .execute("target / identity (source / identity (Identifier \"example\"))")
        .unwrap();
    assert!(execution.error.is_none());
    assert_eq!(
        execution.values,
        vec![Value::Variant {
            shape: "Identifier _".into(),
            arguments: vec![Value::Text("example".into())]
        }]
    );
    assert!(
        interpreter
            .execute("environment / read variable (source / identity (Identifier \"example\"))")
            .is_err()
    );
    interpreter.register("different", Identity(Schema::parse(
        "PROTOCOL different VERSION 1, NAMED TYPE Identifier (Integer), METHOD identity (value: Identifier) RETURNS (value: Identifier)"
    ).unwrap())).unwrap();
    assert!(
        interpreter
            .execute("different / identity (source / identity (Identifier \"example\"))")
            .is_err()
    );
}

#[test]
fn nested_diagnostics_point_to_the_original_document() {
    let (mut interpreter, calls) = interpreter();
    let error = interpreter
        .execute("example / text,\nenvironment / read variable (example / integer)")
        .unwrap_err();
    assert_eq!((error.line, error.column), (2, 30));
    assert!(calls.borrow().is_empty());
}

#[test]
fn generic_records_and_defaults_can_contain_computed_fields() {
    let (mut interpreter, _) = interpreter();
    interpreter.register("generic", Identity(Schema::parse(
        "PROTOCOL generic VERSION 1, RECORD Box of (Item: TYPE) { value: Item, count (Integer) DEFAULT count 7 }, METHOD identity (value: Box of (Text)) RETURNS (value: Box of (Text))"
    ).unwrap())).unwrap();
    let execution = interpreter
        .execute("generic / identity ({ value: (example / text) } AS: Box of (Text))")
        .unwrap();
    assert!(execution.error.is_none());
    let Value::Record(record) = &execution.values[0] else {
        panic!()
    };
    assert_eq!(record["value"], Value::Text("hello".into()));
    assert_eq!(record["count"], Value::Integer(7));
}

#[test]
fn a_field_with_several_slots_takes_a_computed_value_in_each() {
    let (mut interpreter, _) = interpreter();
    interpreter.register("range", Identity(Schema::parse(
        "PROTOCOL range VERSION 1, RECORD Span { between (Text) and (Integer) or (Integer?) }, METHOD identity (value: Span) RETURNS (value: Span)"
    ).unwrap())).unwrap();
    let execution = interpreter
        .execute("range / identity { between (example / text) and 2 or 3 }")
        .unwrap();
    assert!(execution.error.is_none(), "{:?}", execution.error);
    let Value::Record(record) = &execution.values[0] else {
        panic!()
    };
    assert_eq!(
        record["between and or"],
        Value::Tuple(vec![
            Value::Text("hello".into()),
            Value::Integer(2),
            Value::Integer(3),
        ])
    );
    let problem = interpreter
        .execute("range / identity { between \"a\" and \"b\" or 3 }")
        .unwrap_err();
    assert!(
        problem.message.contains("expected a whole number"),
        "{}",
        problem.message
    );
}

#[test]
fn invalid_nested_tool_output_prevents_the_parent_from_running() {
    let (mut interpreter, calls) = interpreter();
    interpreter.register("broken", Identity(Schema::parse(
        "PROTOCOL broken VERSION 1, METHOD identity (value: Text) RETURNS (value: Integer)"
    ).unwrap())).unwrap();
    let execution = interpreter
        .execute("example / handle (broken / identity \"wrong\"), example / text")
        .unwrap();
    assert_eq!(execution.error.as_deref(), Some("INTERNAL ERROR\n"));
    assert!(execution.values.is_empty());
    assert!(calls.borrow().is_empty());
}

#[test]
fn argument_failure_keeps_effects_of_preceding_arguments() {
    let (mut interpreter, calls) = interpreter();
    let execution = interpreter
        .execute("example / pair (example / text) with (environment / read variable \"missing\")")
        .unwrap();
    assert_eq!(
        execution.error.as_deref(),
        Some("ERROR `variable does not exist\n")
    );
    assert_eq!(*calls.borrow(), ["text"]);
    assert!(execution.output.is_empty());
}

#[test]
fn refined_values_cross_protocols_checked_by_each() {
    let (mut interpreter, _) = interpreter();
    interpreter.register("source",Identity(Schema::parse("PROTOCOL source VERSION 1, TYPE ALIAS Count: Integer, TYPE ALIAS Positive: Count . WHERE [0 < _], METHOD identity (value: Positive) RETURNS (value: Positive)").unwrap())).unwrap();
    interpreter.register("target",Identity(Schema::parse("PROTOCOL target VERSION 1, RECORD Earlier { value: Text }, TYPE ALIAS Count: Integer, TYPE ALIAS Positive: Count . WHERE [0 < _], METHOD identity (value: Positive) RETURNS (value: Positive)").unwrap())).unwrap();
    let result = interpreter
        .execute("target / identity (source / identity 1)")
        .unwrap();
    assert!(result.error.is_none());
    assert_eq!(result.values, vec![Value::Integer(1)]);
    interpreter.register("different",Identity(Schema::parse("PROTOCOL different VERSION 1, TYPE ALIAS Count: Integer, TYPE ALIAS Positive: Count . WHERE [10 < _], METHOD identity (value: Positive) RETURNS (value: Positive)").unwrap())).unwrap();
    assert!(
        interpreter
            .execute("different / identity (source / identity 1)")
            .is_err()
    );
}

#[test]
fn inline_refinement_literals_are_checked_before_any_tool_runs() {
    let (mut interpreter, calls) = interpreter();
    interpreter
        .register(
            "checked",
            Identity(Schema::parse(include_str!("../examples/refinements.protocol.anyx")).unwrap()),
        )
        .unwrap();
    for request in [
        r#"checked / short "he""#,
        r#"checked / short ("😀")"#,
        "checked / limit 10",
    ] {
        let result = interpreter.execute(request).unwrap();
        assert!(result.error.is_none(), "{request}: {result:?}");
    }
    let error = interpreter
        .execute("example / text,\nchecked / short (\"hello\")")
        .unwrap_err();
    assert!(error.message.contains("at most 3"), "{error}");
    assert_eq!(error.line, 2);
    assert!(calls.borrow().is_empty());
    let error = interpreter.execute("checked / limit 30").unwrap_err();
    assert!(error.message.contains("less than 30"), "{error}");
}

#[test]
fn computed_refinement_arguments_are_checked_before_the_parent_runs() {
    let (mut interpreter, calls) = interpreter();
    struct Tracked {
        schema: Schema,
        calls: Rc<RefCell<Vec<String>>>,
    }
    impl Tool for Tracked {
        fn schema(&self) -> &Schema {
            &self.schema
        }
        fn invoke(&mut self, call: &Call) -> Outcome {
            self.calls.borrow_mut().push("parent".into());
            Outcome::Value(call.arguments[0].clone())
        }
    }
    interpreter
        .register(
            "checked",
            Tracked {
                schema: Schema::parse(include_str!("../examples/refinements.protocol.anyx"))
                    .unwrap(),
                calls: calls.clone(),
            },
        )
        .unwrap();
    let result = interpreter
        .execute("checked / short (example / text)")
        .unwrap();
    // The source returns `hello`, which violates the consuming parameter.
    assert_eq!(result.error.as_deref(), Some("INTERNAL ERROR\n"));
    assert!(result.values.is_empty());
    assert_eq!(*calls.borrow(), ["text"]);
}

#[test]
fn refinement_overloads_resolve_using_base_types() {
    let (mut interpreter, _) = interpreter();
    interpreter
        .register(
            "checked",
            Identity(
                Schema::parse(
                    r#"PROTOCOL checked VERSION 1,
METHOD use (value: Text . WHERE [1 <= (_ length)]) RETURNS (value: Text),
METHOD use (value: Integer . WHERE [0 < _]) RETURNS (value: Integer),
"#,
                )
                .unwrap(),
            ),
        )
        .unwrap();
    for request in [r#"checked / use "he""#, "checked / use 1"] {
        assert!(
            interpreter.execute(request).unwrap().error.is_none(),
            "{request}"
        );
    }
}

fn register_example(interpreter: &mut Interpreter, scope: &str, methods: &str) {
    interpreter
        .register(
            scope,
            Example {
                schema: Schema::parse(&format!("PROTOCOL {scope} VERSION 1, {methods}")).unwrap(),
                calls: Rc::default(),
            },
        )
        .unwrap();
}

#[test]
fn opening_a_tool_is_sequential_and_persists_after_a_valid_submission() {
    let (mut interpreter, calls) = interpreter();
    assert!(interpreter.execute("text, USE ALL FROM example").is_err());
    assert!(
        interpreter
            .execute("USE ALL FROM example, missing")
            .is_err()
    );
    assert!(interpreter.execute("text").is_err());
    assert!(
        interpreter
            .execute("USE ALL FROM unknown")
            .unwrap_err()
            .message
            .contains("unknown tool scope")
    );
    assert!(
        interpreter
            .execute(r#"USE ALL FROM "./example.protocol.anyx""#)
            .is_err()
    );
    assert!(calls.borrow().is_empty());

    let values = interpreter
        .execute(
            "USE ALL FROM example, USE ALL FROM example, handle (text), example / handle (integer)",
        )
        .unwrap()
        .values;
    assert_eq!(
        values,
        [Value::Text("Text".into()), Value::Text("Integer".into())]
    );
    assert_eq!(
        interpreter.execute("integer").unwrap().values,
        [Value::Integer(42)]
    );
}

#[test]
fn opened_calls_use_argument_and_expected_return_types() {
    let (mut interpreter, _) = interpreter();
    register_example(
        &mut interpreter,
        "first",
        "METHOD pick (x: Text) RETURNS (value: Text), METHOD text RETURNS (value: Text)",
    );
    register_example(
        &mut interpreter,
        "second",
        "METHOD pick (x: Integer) RETURNS (value: Integer), METHOD text RETURNS (value: Integer)",
    );
    interpreter
        .execute("USE ALL FROM first, USE ALL FROM second")
        .unwrap();

    assert_eq!(
        interpreter
            .execute(r#"pick 42, pick "hi", first / pick (text)"#)
            .unwrap()
            .values,
        [
            Value::Integer(42),
            Value::Text("hi".into()),
            Value::Text("hello".into())
        ]
    );
    let error = interpreter.execute("text").unwrap_err().message;
    assert!(error.contains("ambiguous call"), "{error}");
    assert!(
        error.contains("first / text") && error.contains("second / text"),
        "{error}"
    );
}

#[test]
fn opened_mixfix_calls_follow_anyx_grouping() {
    let mut interpreter = Interpreter::new();
    interpreter
        .register("boolean", anyx_shell::boolean::Boolean)
        .unwrap();
    let execution = interpreter
        .execute("USE ALL FROM boolean, `true .and (`false .and (`true))")
        .unwrap();
    assert_eq!(execution.output, "`false\n");
    let source = "`true .an";
    assert!(
        interpreter
            .completions(source, source.len())
            .items
            .iter()
            .any(|item| item.insert_text.starts_with("and ")
                && item.detail.starts_with("boolean / "))
    );

    assert!(
        interpreter
            .execute("`true and (`false) and (`true)")
            .is_err()
    );
}

#[test]
fn completion_suggests_opened_methods_and_qualifies_collisions() {
    let (mut interpreter, _) = interpreter();
    register_example(
        &mut interpreter,
        "other",
        "METHOD text RETURNS (value: Text)",
    );
    let source = "USE ALL FROM example, example / handle (te";
    let completions = interpreter.completions(source, source.len());
    assert!(
        completions
            .items
            .iter()
            .any(|item| item.insert_text == "text")
    );

    interpreter
        .execute("USE ALL FROM example, USE ALL FROM other")
        .unwrap();
    let completions = interpreter.completions("te", 2);
    assert!(
        completions
            .items
            .iter()
            .any(|item| item.insert_text == "example / text")
    );
    assert!(
        completions
            .items
            .iter()
            .any(|item| item.insert_text == "other / text")
    );
}
