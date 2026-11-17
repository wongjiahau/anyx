//! A non-interactive interpreter for calls to scoped anyx protocols.
//!
//! Each top-level expression is `scope / method arguments`. The interpreter
//! validates every call before invoking any tool, then executes in order.
//! Successful results are standalone anyx values; protocol envelopes are not
//! displayed. A method failure stops execution without rolling back earlier calls.

mod compile;
pub mod environment;

use std::collections::BTreeMap;

use anyx::line_column;
use anyx_protocol::{Call, Outcome, Problem, Response, Schema, Value};
use compile::{Argument, Call as PreparedCall, Compiler, ExpressionValue};

/// A session-owned tool. Calls have already been checked against `schema()`.
/// Implementations must keep their schema stable for their lifetime.
pub trait Tool {
    fn schema(&self) -> &Schema;
    fn invoke(&mut self, call: &Call) -> Outcome;
}

#[derive(Default)]
pub struct Interpreter {
    tools: BTreeMap<String, Box<dyn Tool>>,
}

/// Results up to the first execution failure. `output` is an anyx document
/// containing only returned values. `error`, if present, is an anyx protocol
/// error expression for standard error. Earlier successful effects remain.
#[derive(Debug, Default)]
pub struct Execution {
    pub values: Vec<Value>,
    pub output: String,
    pub error: Option<String>,
}

impl Interpreter {
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a protocol under one lowercase word. Registration never replaces
    /// an existing tool. The shell owns scopes; protocol method names stay local.
    pub fn register(&mut self, scope: &str, tool: impl Tool + 'static) -> Result<(), String> {
        let mut characters = scope.chars();
        if !characters.next().is_some_and(|c| c.is_ascii_lowercase())
            || !characters.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        {
            return Err("a tool scope must start with a lowercase letter and contain only lowercase letters and digits".into());
        }
        if self.tools.contains_key(scope) {
            return Err(format!("the tool scope `{scope}` is already registered"));
        }
        if !tool.schema().is_protocol() {
            return Err("a tool must expose a protocol with a PROTOCOL header".into());
        }
        self.tools.insert(scope.into(), Box::new(tool));
        Ok(())
    }

    /// Prepares the whole document before changing session state. A syntax,
    /// scope, method or argument error means none of the calls run.
    pub fn execute(&mut self, source: &str) -> Result<Execution, Problem> {
        let parsed = anyx::parse(source);
        if let Some(error) = parsed.errors.first() {
            return Err(at(source, error.span.start, error.message.clone()));
        }
        let compiler = Compiler {
            interpreter: self,
            source,
        };
        let prepared = parsed
            .syntax
            .sequence
            .elements
            .iter()
            .map(|element| compiler.element(element))
            .collect::<Result<Vec<_>, _>>()?;
        let mut execution = Execution::default();
        let mut written = Vec::new();
        for prepared in prepared {
            match self.evaluate_call(&prepared) {
                Ok(Value::Nothing) => {}
                Ok(value) => {
                    let schema = self.tools[&prepared.scope].schema();
                    let returns = schema
                        .method_at(prepared.method)
                        .returns
                        .as_ref()
                        .expect("a checked value has a return type");
                    match anyx_protocol::encode_value(schema, &returns.r#type, &value) {
                        Ok(text) => {
                            written.push(text);
                            execution.values.push(value);
                        }
                        Err(_) => {
                            execution.error = Some("INTERNAL ERROR\n".into());
                            break;
                        }
                    }
                }
                Err(error) => {
                    execution.error = Some(error);
                    break;
                }
            }
        }
        if !written.is_empty() {
            let document = written
                .iter()
                .map(|value| value.trim_end())
                .collect::<Vec<_>>()
                .join(",\n");
            execution.output =
                anyx::format(&document).expect("encoded values form an anyx document");
        }
        Ok(execution)
    }

    fn evaluate_call(&mut self, prepared: &PreparedCall) -> Result<Value, String> {
        let arguments = prepared
            .arguments
            .iter()
            .map(|argument| self.evaluate(argument))
            .collect::<Result<Vec<_>, _>>()?;
        let call = Call {
            method: prepared.method,
            arguments,
        };
        let tool = self
            .tools
            .get_mut(&prepared.scope)
            .expect("prepared scope exists");
        let schema = tool.schema();
        let outcome = if call.method == schema.introspection() {
            anyx_protocol::server::introspect(schema)
        } else {
            tool.invoke(&call)
        };
        let schema = tool.schema();
        let response = Response::Outcomes(vec![outcome]);
        let wire = anyx_protocol::encode_response(schema, std::slice::from_ref(&call), &response)
            .map_err(|_| "INTERNAL ERROR\n".to_string())?;
        // Decode the response to validate the tool output and materialize defaults.
        let response = anyx_protocol::decode_response(schema, &[call.method], &wire)
            .map_err(|_| "INTERNAL ERROR\n".to_string())?;
        match response {
            Response::Outcomes(mut outcomes) => match outcomes.remove(0) {
                Outcome::Value(value) => Ok(value),
                Outcome::Error(_) => Err(wire),
                _ => Err("INTERNAL ERROR\n".into()),
            },
            _ => Err("INTERNAL ERROR\n".into()),
        }
    }

    fn evaluate(&mut self, argument: &Argument) -> Result<Value, String> {
        Ok(match &argument.value {
            ExpressionValue::Literal(value) => value.clone(),
            ExpressionValue::Call(call) => self.evaluate_call(call)?,
            ExpressionValue::List(items) => Value::List(
                items
                    .iter()
                    .map(|item| self.evaluate(item))
                    .collect::<Result<_, _>>()?,
            ),
            ExpressionValue::Record(fields) => Value::Record(
                fields
                    .iter()
                    .map(|(name, value)| Ok((name.clone(), self.evaluate(value)?)))
                    .collect::<Result<_, String>>()?,
            ),
            ExpressionValue::Map(fields) => Value::Map(
                fields
                    .iter()
                    .map(|(name, value)| Ok((name.clone(), self.evaluate(value)?)))
                    .collect::<Result<_, String>>()?,
            ),
            ExpressionValue::Variant(shape, arguments) => Value::Variant {
                shape: shape.clone(),
                arguments: arguments
                    .iter()
                    .map(|argument| self.evaluate(argument))
                    .collect::<Result<_, _>>()?,
            },
        })
    }
}

fn at(source: &str, offset: usize, message: String) -> Problem {
    let position = line_column(source, offset);
    Problem {
        line: position.line,
        column: position.column,
        message,
    }
}

/// An anyx diagnostic for errors in source, invocation or input/output.
pub fn diagnostic(problem: Problem) -> String {
    // An invalid-request envelope uses no method or type from this schema.
    anyx_protocol::encode_response(
        environment::schema(),
        &[],
        &Response::InvalidRequest(problem),
    )
    .expect("a diagnostic always has an anyx representation")
}
