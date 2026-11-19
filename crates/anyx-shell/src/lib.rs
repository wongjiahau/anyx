//! An interpreter for calls to scoped anyx protocols.
//!
//! Calls can qualify a method with its scope or use an opened tool. The interpreter
//! validates every call before invoking any tool, then executes in order.
//! Successful results are standalone anyx values; protocol envelopes are not
//! displayed. A method failure stops execution without rolling back earlier calls.

mod compile;
mod completion;
mod hover;
pub use completion::{Completion, Completions, TabStop};
pub use hover::ArgumentInfo;
pub mod boolean;
pub mod command;
pub mod environment;
#[allow(unused_imports)]
mod shell_grammar;

use std::collections::{BTreeMap, BTreeSet};

use anyx::line_column;
use anyx_protocol::{Call, Outcome, Problem, Response, Schema, Value};
use compile::{Argument, Call as PreparedCall, Compiler, ExpressionValue};

enum PreparedStatement {
    Opening(String),
    Call(PreparedCall),
}

/// A session-owned tool. Calls have already been checked against `schema()`.
/// Implementations must keep their schema stable for their lifetime.
pub trait Tool {
    fn schema(&self) -> &Schema;
    fn invoke(&mut self, call: &Call) -> Outcome;
}

#[derive(Default)]
pub struct Interpreter {
    tools: BTreeMap<String, Box<dyn Tool>>,
    opened: BTreeSet<String>,
    /// The program each tool brought in with `USE … FROM COMMAND` runs.
    commands: BTreeMap<String, String>,
}

/// A tool brought in while preparing a document, and the tool it replaced,
/// to put back if the document fails.
struct Staged {
    scope: String,
    program: String,
    replaced: Option<(Box<dyn Tool>, String)>,
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
    /// scope, method or argument error means none of the calls run, and no
    /// tool it brings in stays.
    pub fn execute(&mut self, source: &str) -> Result<Execution, Problem> {
        let parsed = anyx::parse(source);
        if let Some(error) = parsed.errors.first() {
            return Err(at(source, error.span.start, error.message.clone()));
        }
        let mut staged = Vec::new();
        let prepared = match self.prepare(source, &parsed.syntax.nodes(), &mut staged) {
            Ok(prepared) => prepared,
            Err(problem) => {
                for staged in staged.into_iter().rev() {
                    self.tools.remove(&staged.scope);
                    self.commands.remove(&staged.scope);
                    if let Some((tool, program)) = staged.replaced {
                        self.tools.insert(staged.scope.clone(), tool);
                        self.commands.insert(staged.scope, program);
                    }
                }
                return Err(problem);
            }
        };
        self.run(source, prepared)
    }

    fn prepare(
        &mut self,
        source: &str,
        nodes: &[anyx::Node],
        staged: &mut Vec<Staged>,
    ) -> Result<Vec<PreparedStatement>, Problem> {
        let mut opened = self.opened.clone();
        let mut prepared = Vec::new();
        for node in nodes {
            match shell_grammar::decode_directive(node) {
                Ok(shell_grammar::Directive::Opening(opening)) => {
                    let scope = opening.scope.name.text;
                    if !self.tools.contains_key(&scope) {
                        return Err(at(
                            source,
                            opening.scope.span.start,
                            format!("unknown tool scope `{scope}`"),
                        ));
                    }
                    opened.insert(scope.clone());
                    prepared.push(PreparedStatement::Opening(scope));
                }
                Ok(shell_grammar::Directive::Import(import)) => {
                    let scope = import.scope.name.text;
                    let program = import.program.value.value;
                    staged.push(
                        self.import(&scope, &program)
                            .map_err(|message| at(source, import.span.start, message))?,
                    );
                }
                Err(_) => {
                    let compiler = Compiler {
                        interpreter: self,
                        source,
                        opened: &opened,
                    };
                    prepared.push(PreparedStatement::Call(compiler.element(node)?));
                }
            }
        }
        Ok(prepared)
    }

    /// Brings in `program` as the tool `scope`. Bringing in the same program
    /// under the same name again asks it for its schema again.
    fn import(&mut self, scope: &str, program: &str) -> Result<Staged, String> {
        let replaced = match self.commands.get(scope) {
            Some(existing) if existing == program => {
                let tool = self.tools.remove(scope).expect("a command has a tool");
                Some((tool, self.commands.remove(scope).expect("checked")))
            }
            Some(existing) => {
                return Err(format!(
                    "the tool scope `{scope}` is already the program `{existing}`"
                ));
            }
            None if self.tools.contains_key(scope) => {
                return Err(format!("the tool scope `{scope}` is already registered"));
            }
            None => None,
        };
        let staged = Staged {
            scope: scope.to_string(),
            program: program.to_string(),
            replaced,
        };
        let tool = match command::CommandTool::open(program) {
            Ok(tool) => tool,
            Err(message) => {
                if let Some((tool, program)) = staged.replaced {
                    self.tools.insert(scope.to_string(), tool);
                    self.commands.insert(scope.to_string(), program);
                }
                return Err(message);
            }
        };
        if let Err(message) = self.register(scope, tool) {
            if let Some((tool, program)) = staged.replaced {
                self.tools.insert(scope.to_string(), tool);
                self.commands.insert(scope.to_string(), program);
            }
            return Err(message);
        }
        self.commands
            .insert(staged.scope.clone(), staged.program.clone());
        Ok(staged)
    }

    fn run(
        &mut self,
        source: &str,
        prepared: Vec<PreparedStatement>,
    ) -> Result<Execution, Problem> {
        let mut execution = Execution::default();
        let mut written = Vec::new();
        for prepared in prepared {
            let prepared = match prepared {
                PreparedStatement::Opening(scope) => {
                    self.opened.insert(scope);
                    continue;
                }
                PreparedStatement::Call(call) => call,
            };
            match self.evaluate_call(source, &prepared) {
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

    fn evaluate_call(&mut self, source: &str, prepared: &PreparedCall) -> Result<Value, String> {
        let arguments = prepared
            .arguments
            .iter()
            .map(|argument| self.evaluate(source, argument))
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
        // Computed arguments become available only now. Check them before
        // invoking the consuming tool, just as literals were checked at compile time.
        for (parameter, value) in schema
            .method_at(call.method)
            .shape
            .parameters
            .iter()
            .zip(&call.arguments)
        {
            anyx_protocol::refinement::validate(schema, &parameter.r#type, value)
                .map_err(|_| "INTERNAL ERROR\n".to_string())?;
        }
        // The call is the shell's, so a `REQUIRES` its computed arguments
        // fail is a mistake in the document, not in a tool.
        anyx_protocol::refinement::check_precondition(
            schema,
            schema.method_at(call.method),
            &call.arguments,
        )
        .map_err(|message| diagnostic(at(source, prepared.span.start, message)))?;
        let outcome = match anyx_protocol::server::built_in(schema, &call) {
            Some(outcome) => outcome,
            None => tool.invoke(&call),
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

    fn evaluate(&mut self, source: &str, argument: &Argument) -> Result<Value, String> {
        Ok(match &argument.value {
            ExpressionValue::Literal(value) => value.clone(),
            ExpressionValue::Call(call) => self.evaluate_call(source, call)?,
            ExpressionValue::List(items) => Value::List(
                items
                    .iter()
                    .map(|item| self.evaluate(source, item))
                    .collect::<Result<_, _>>()?,
            ),
            ExpressionValue::Record(fields) => Value::Record(
                fields
                    .iter()
                    .map(|(name, value)| Ok((name.clone(), self.evaluate(source, value)?)))
                    .collect::<Result<_, String>>()?,
            ),
            ExpressionValue::Map(fields) => Value::Map(
                fields
                    .iter()
                    .map(|(name, value)| Ok((name.clone(), self.evaluate(source, value)?)))
                    .collect::<Result<_, String>>()?,
            ),
            ExpressionValue::Variant(shape, arguments) => Value::Variant {
                shape: shape.clone(),
                arguments: arguments
                    .iter()
                    .map(|argument| self.evaluate(source, argument))
                    .collect::<Result<_, _>>()?,
            },
            ExpressionValue::Tuple(slots) => Value::Tuple(
                slots
                    .iter()
                    .map(|slot| self.evaluate(source, slot))
                    .collect::<Result<_, _>>()?,
            ),
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
