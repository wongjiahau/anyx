//! Reads and writes responses: one result per call, in the order of the
//! calls, which a reader matches to its call by position. A result doesn't
//! repeat its call, so no argument, such as a password, comes back:
//!
//! ```anyx
//! RETURNS { path "a.poentaro", revision 3, content "…" },
//! ERROR `missing,
//! ```
//!
//! A result is `RETURNS` and the method's value (`RETURNS NOTHING` for a
//! method that returns nothing), `ERROR` and one of its errors, `INTERNAL
//! ERROR` or `NOT RUN`. A request that can't be read gets the single
//! response `INVALID REQUEST { … }` instead.

use anyx::{NamePart, Node, NodeKind, Span};

use crate::codec::{
    Call, EncodeError, Mismatch, Outcome, Problem, Response, decode, decode_variant, describe,
    layout, parse, problem, write, write_shape, write_text,
};
use crate::schema::{Method, Schema, Shape};
use crate::value::Value;

/// Writes the response to `calls`, one result each, in their order.
pub fn encode_response(
    schema: &Schema,
    calls: &[Call],
    response: &Response,
) -> Result<String, EncodeError> {
    let outcomes = match response {
        Response::InvalidRequest(problem) => {
            let mut out = String::from("INVALID REQUEST { line ");
            out.push_str(&problem.line.to_string());
            out.push_str(", column ");
            out.push_str(&problem.column.to_string());
            out.push_str(", message ");
            write_text(&problem.message, &mut out);
            out.push_str(" }\n");
            return Ok(layout(out));
        }
        Response::Outcomes(outcomes) => outcomes,
    };
    let written = calls
        .iter()
        .zip(outcomes)
        .map(|(call, outcome)| {
            let method = schema.method_at(call.method);
            let written = encode_outcome(schema, method, outcome)?;
            match check_postcondition(schema, call, outcome) {
                Ok(()) => Ok(written),
                Err(_) => Ok("INTERNAL ERROR".into()),
            }
        })
        .collect::<Result<Vec<_>, EncodeError>>()?;
    Ok(match written.as_slice() {
        [outcome] => format!("{outcome}\n"),
        outcomes => outcomes
            .iter()
            .map(|outcome| format!("{outcome},\n"))
            .collect(),
    })
}

pub(crate) fn validate_outcome(
    schema: &Schema,
    call: &Call,
    outcome: &Outcome,
) -> Result<(), EncodeError> {
    encode_outcome(schema, schema.method_at(call.method), outcome)?;
    check_postcondition(schema, call, outcome)
}

fn check_postcondition(schema: &Schema, call: &Call, outcome: &Outcome) -> Result<(), EncodeError> {
    crate::refinement::check_postcondition(
        schema,
        schema.method_at(call.method),
        &call.arguments,
        outcome,
    )
    .map_err(|message| {
        eprintln!("{message}");
        EncodeError { message }
    })
}

/// One result of a call of `method`, laid out the way `anyx::format` lays
/// it out.
fn encode_outcome(
    schema: &Schema,
    method: &Method,
    outcome: &Outcome,
) -> Result<String, EncodeError> {
    let mut out = String::new();
    match outcome {
        Outcome::Value(value) => match (&method.returns, value) {
            (Some(returns), _) => {
                out.push_str("RETURNS ");
                write(schema, &returns.r#type, value, &mut out)?;
            }
            (None, Value::Nothing) => out.push_str("RETURNS NOTHING"),
            (None, _) => {
                return Err(EncodeError {
                    message: format!(
                        "found a value returned by `{}`; expected nothing, since it `RETURNS NOTHING`",
                        method.full_name()
                    ),
                });
            }
        },
        Outcome::Error(error) => {
            out.push_str("ERROR ");
            let Value::Variant { shape, arguments } = error else {
                return Err(EncodeError {
                    message: "found an error that is not a variant".to_string(),
                });
            };
            let Some(error) = schema
                .errors_of(method)
                .into_iter()
                .find(|error| error.shape.text == *shape)
            else {
                return Err(EncodeError {
                    message: format!(
                        "found the error `{shape}`; expected an error of `{}`",
                        method.full_name()
                    ),
                });
            };
            out.push('`');
            write_shape(schema, &error.shape, arguments, &mut out)?;
        }
        Outcome::NotRun => out.push_str("NOT RUN"),
        Outcome::InternalError => out.push_str("INTERNAL ERROR"),
    }
    Ok(layout(out).trim_end().to_string())
}

/// Reads a response to calls of `methods`: one result per call, matched to
/// its call by position.
pub fn decode_response(
    schema: &Schema,
    methods: &[usize],
    source: &str,
) -> Result<Response, Problem> {
    let nodes = parse(source)?;
    if let [node] = &nodes[..]
        && let NodeKind::Call(call) = &node.kind
        && call.name_text() == "INVALID REQUEST _"
    {
        return invalid_request(&call.arguments[0])
            .map(Response::InvalidRequest)
            .ok_or_else(|| {
                problem(
                    source,
                    node.span,
                    "found an `INVALID REQUEST` without a line, column and message".to_string(),
                )
            });
    }
    if nodes.len() != methods.len() {
        let span = nodes
            .get(methods.len())
            .map_or(Span::empty(source.len()), |extra| extra.span);
        return Err(problem(
            source,
            span,
            format!(
                "found {}; expected {}, one per call, in the order of the calls",
                count(nodes.len(), "result"),
                count(methods.len(), "result"),
            ),
        ));
    }
    methods
        .iter()
        .zip(&nodes)
        .enumerate()
        .map(|(index, (method, node))| {
            let method = schema.method_at(*method);
            decode_outcome(schema, method, node).map_err(|mismatch| {
                let message = match methods.len() {
                    1 => mismatch.message,
                    total => format!(
                        "in result {} of {total}, for the call of `{}`: {}",
                        index + 1,
                        method.full_name(),
                        mismatch.message
                    ),
                };
                problem(source, mismatch.span, message)
            })
        })
        .collect::<Result<_, _>>()
        .map(Response::Outcomes)
}

/// `count` of `noun`, such as "1 result" or "2 results".
fn count(count: usize, noun: &str) -> String {
    match count {
        1 => format!("1 {noun}"),
        count => format!("{count} {noun}s"),
    }
}

/// A result as written, in a response or after the call in an example.
pub(crate) enum WrittenOutcome<'n> {
    Returns(&'n Node),
    ReturnsNothing,
    Error(&'n Node),
    NotRun,
    InternalError,
}

fn written_outcome(node: &Node) -> Option<WrittenOutcome<'_>> {
    let NodeKind::Call(call) = &node.kind else {
        return None;
    };
    let word = |part: &NamePart, expected: &str| matches!(part, NamePart::Word { text, .. } if text == expected);
    match (call.name.as_slice(), call.arguments.as_slice()) {
        ([not, run], []) if word(not, "NOT") && word(run, "RUN") => Some(WrittenOutcome::NotRun),
        ([returns, nothing], []) if word(returns, "RETURNS") && word(nothing, "NOTHING") => {
            Some(WrittenOutcome::ReturnsNothing)
        }
        ([internal, error], []) if word(internal, "INTERNAL") && word(error, "ERROR") => {
            Some(WrittenOutcome::InternalError)
        }
        ([returns, NamePart::Slot], [value]) if word(returns, "RETURNS") => {
            Some(WrittenOutcome::Returns(value))
        }
        ([error, NamePart::Slot], [value]) if word(error, "ERROR") => {
            Some(WrittenOutcome::Error(value))
        }
        _ => None,
    }
}

/// Whether `node` is a call followed by a result, as an example writes it:
/// `read "a" RETURNS …`.
fn repeats_a_call(node: &Node) -> bool {
    let NodeKind::Call(call) = &node.kind else {
        return false;
    };
    let keyword = |part: &NamePart| matches!(part, NamePart::Word { text, .. } if ["RETURNS", "ERROR", "NOT", "INTERNAL"].contains(&text.as_str()));
    call.name
        .iter()
        .position(keyword)
        .is_some_and(|position| position > 0)
}

fn decode_outcome(schema: &Schema, method: &Method, node: &Node) -> Result<Outcome, Mismatch> {
    let Some(written) = written_outcome(node) else {
        let returns = match method.returns {
            Some(_) => "`RETURNS` and its value",
            None => "`RETURNS NOTHING`",
        };
        let hint = match repeats_a_call(node) {
            true => "; a result doesn't repeat its call, so write it alone",
            false => "",
        };
        return Err(Mismatch {
            span: node.span,
            message: format!(
                "found {}; expected the result of `{}`, the call in that place of the request: {returns}, `ERROR` and one of its errors, `INTERNAL ERROR` or `NOT RUN`{hint}",
                describe(node),
                method.full_name()
            ),
        });
    };
    read_outcome(schema, method, written, node.span)
}

/// Reads a result of a call of `method`; `span` is the result's.
pub(crate) fn read_outcome(
    schema: &Schema,
    method: &Method,
    written: WrittenOutcome,
    span: Span,
) -> Result<Outcome, Mismatch> {
    match written {
        WrittenOutcome::NotRun => Ok(Outcome::NotRun),
        WrittenOutcome::InternalError => Ok(Outcome::InternalError),
        WrittenOutcome::Error(error) if method.errors.is_empty() => Err(Mismatch {
            span: error.span,
            message: format!(
                "found an error after `ERROR`; expected `RETURNS` and what `{}` returns, since it has no errors",
                method.full_name()
            ),
        }),
        WrittenOutcome::Error(error) => {
            let errors = schema.errors_of(method);
            let shapes: Vec<&Shape> = errors.iter().map(|error| &error.shape).collect();
            decode_variant(
                schema,
                &shapes,
                error,
                &format!("an error of `{}`", method.full_name()),
            )
            .map(Outcome::Error)
        }
        WrittenOutcome::ReturnsNothing => match &method.returns {
            None => Ok(Outcome::Value(Value::Nothing)),
            Some(returns) => Err(Mismatch {
                span,
                message: format!(
                    "found `RETURNS NOTHING`; expected `RETURNS` and its `{}`, which `{}` returns",
                    returns.name,
                    method.full_name()
                ),
            }),
        },
        WrittenOutcome::Returns(value) => {
            let Some(returns) = &method.returns else {
                return Err(Mismatch {
                    span: value.span,
                    message: format!(
                        "found a value after `RETURNS`; expected `RETURNS NOTHING`, since `{}` returns nothing",
                        method.full_name()
                    ),
                });
            };
            decode(schema, &returns.r#type, value)
                .map(Outcome::Value)
                .map_err(|mismatch| match &value.kind {
                    NodeKind::Call(call)
                        if schema
                            .errors_of(method)
                            .iter()
                            .any(|error| error.shape.text == call.name_text()) =>
                    {
                        Mismatch {
                            message: format!(
                                "{}; an error is written after `ERROR`, as in `ERROR {}`",
                                mismatch.message,
                                call.name_text().replace(" _", " …")
                            ),
                            ..mismatch
                        }
                    }
                    _ => mismatch,
                })
        }
    }
}

fn invalid_request(node: &Node) -> Option<Problem> {
    let NodeKind::Braces(nodes) = &node.kind else {
        return None;
    };
    let mut line = None;
    let mut column = None;
    let mut message = None;
    for node in nodes {
        let NodeKind::Call(call) = &node.kind else {
            return None;
        };
        match (call.name_text().as_str(), &call.arguments.first()?.kind) {
            ("line _", NodeKind::Number(text)) => line = text.parse().ok(),
            ("column _", NodeKind::Number(text)) => column = text.parse().ok(),
            ("message _", NodeKind::String(literal)) => message = Some(literal.value.clone()),
            _ => return None,
        }
    }
    Some(Problem {
        line: line?,
        column: column?,
        message: message?,
    })
}
