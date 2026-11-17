//! A non-interactive interpreter for calls to scoped anyx protocols.
//!
//! Each top-level expression is `scope / method arguments`. The interpreter
//! validates every call before invoking any tool, then executes in order.
//! Successful results are standalone anyx values; protocol envelopes are not
//! displayed. A method failure stops execution without rolling back earlier calls.

pub mod environment;

use std::collections::BTreeMap;

use anyx::{Element, Expression, Item, TokenKind, line_column};
use anyx_protocol::{Call, Outcome, Problem, Response, Schema, Value};

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

struct Prepared {
    scope: String,
    call: Call,
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
        let prepared = parsed
            .syntax
            .sequence
            .elements
            .iter()
            .map(|element| self.prepare(source, element))
            .collect::<Result<Vec<_>, _>>()?;
        let mut execution = Execution::default();
        let mut written = Vec::new();
        for prepared in prepared {
            let tool = self
                .tools
                .get_mut(&prepared.scope)
                .expect("prepared scope exists");
            let schema = tool.schema();
            let outcome = if prepared.call.method == schema.introspection() {
                anyx_protocol::server::introspect(schema)
            } else {
                tool.invoke(&prepared.call)
            };
            let schema = tool.schema();
            // Check the complete outcome, including declared method errors, before
            // displaying it. Invalid tool output is an internal failure.
            let response = Response::Outcomes(vec![outcome.clone()]);
            let wire = anyx_protocol::encode_response(
                schema,
                std::slice::from_ref(&prepared.call),
                &response,
            );
            let wire = wire.and_then(|wire| {
                anyx_protocol::decode_response(schema, &[prepared.call.method], &wire).map_err(
                    |problem| anyx_protocol::EncodeError {
                        message: problem.message,
                    },
                )?;
                Ok(wire)
            });
            match (outcome, wire) {
                (Outcome::Value(Value::Nothing), Ok(_)) => {}
                (Outcome::Value(value), Ok(_)) => {
                    let returns = schema
                        .method_at(prepared.call.method)
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
                (Outcome::Error(_), Ok(wire)) => {
                    execution.error = Some(wire);
                    break;
                }
                _ => {
                    execution.error = Some("INTERNAL ERROR\n".into());
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

    fn prepare(&self, source: &str, element: &Element) -> Result<Prepared, Problem> {
        // Read only the shell prefix from the lossless tree. In particular,
        // `environment / SCHEMA` has a different abstract grouping because
        // uppercase words bind looser than `/`. The protocol owns the body.
        let Expression::Run(run) = &element.expression else {
            return Err(at(
                source,
                expression_start(&element.expression),
                "expected `scope / method arguments`; top-level chaining is not supported yet"
                    .into(),
            ));
        };
        let fail = |message| at(source, run.position, message);
        let [Item::Atom(scope), Item::Atom(slash), ..] = run.items.as_slice() else {
            return Err(fail(
                "expected a scoped call, such as `environment / list variables`".into(),
            ));
        };
        if scope.kind != TokenKind::Word || slash.kind != TokenKind::Word || slash.text != "/" {
            return Err(fail(
                "expected a scoped call, such as `environment / list variables`".into(),
            ));
        }
        let tool = self
            .tools
            .get(&scope.text)
            .ok_or_else(|| fail(format!("unknown tool scope `{}`", scope.text)))?;
        let start = slash.span.end;
        let end = element
            .comma
            .as_ref()
            .map_or(source.len(), |comma| comma.span.start);
        let calls = anyx_protocol::decode_request(tool.schema(), &source[start..end]).map_err(
            |problem| {
                let base = line_column(source, start);
                Problem {
                    line: base.line + problem.line - 1,
                    column: if problem.line == 1 {
                        base.column + problem.column - 1
                    } else {
                        problem.column
                    },
                    message: problem.message,
                }
            },
        )?;
        let [call] = calls.as_slice() else {
            return Err(fail("expected exactly one method call after `/`".into()));
        };
        Ok(Prepared {
            scope: scope.text.clone(),
            call: call.clone(),
        })
    }
}

fn expression_start(expression: &Expression) -> usize {
    match expression {
        Expression::Run(run) => run.position,
        Expression::Dot(binary) | Expression::Colon(binary) => expression_start(&binary.left),
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
