//! Reads and writes requests, and the values in them and in responses
//! (`response`), checking each value against the schema.
//!
//! A request is an anyx document of calls, each written in its method's
//! shape: `read "Hymns/a.poentaro"`.

use std::collections::BTreeMap;

use anyx::{Node, NodeKind, Span, line_column};

use crate::annotation::{own_syntax_type, own_type, request_type};
use crate::request_grammar as request;
use crate::schema::{Field, FieldForm, Method, Schema, Shape, Type, TypeKind};
use crate::value::Value;

/// One call of a request: which method, and its arguments in order.
#[derive(Clone, Debug, PartialEq)]
pub struct Call {
    /// The index of the method in [`Schema::methods`].
    pub method: usize,
    pub arguments: Vec<Value>,
}

/// What came of one call.
#[derive(Clone, Debug, PartialEq)]
pub enum Outcome {
    /// What the method returns.
    Value(Value),
    /// One of the method's errors, as a [`Value::Variant`].
    Error(Value),
    /// An earlier call hit an internal error, so this one never ran.
    NotRun,
    /// The server failed. Nothing about the failure is sent; the server logs it.
    InternalError,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Response {
    /// One outcome per call, in order.
    Outcomes(Vec<Outcome>),
    /// The request couldn't be read, so nothing ran.
    InvalidRequest(Problem),
}

/// Where a document went wrong, counted from 1 (the column in characters).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Problem {
    pub line: usize,
    pub column: usize,
    pub message: String,
}

impl std::fmt::Display for Problem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}: {}", self.line, self.column, self.message)
    }
}

impl std::error::Error for Problem {}

/// A value that has no form on the wire, such as a `Float` that is not
/// finite.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EncodeError {
    pub message: String,
}

impl std::fmt::Display for EncodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for EncodeError {}

pub(crate) struct Mismatch {
    pub(crate) span: Span,
    pub(crate) message: String,
}

pub(crate) fn problem(source: &str, span: Span, message: String) -> Problem {
    let position = line_column(source, span.start);
    Problem {
        line: position.line,
        column: position.column,
        message,
    }
}

pub(crate) fn parse(source: &str) -> Result<Vec<Node>, Problem> {
    let parsed = anyx::parse(source);
    match parsed.errors.into_iter().next() {
        Some(error) => Err(problem(source, error.span, error.message)),
        None => Ok(parsed.syntax.nodes()),
    }
}

/// Reads a request: every call must name a method and fit its parameters.
pub fn decode_request(schema: &Schema, source: &str) -> Result<Vec<Call>, Problem> {
    let nodes = parse(source)?;
    if nodes.is_empty() {
        return Err(problem(
            source,
            Span::empty(0),
            format!(
                "found an empty request; expected at least one call, such as `{}`",
                schema
                    .methods
                    .first()
                    .map_or("a method".to_string(), |method| method.full_name())
            ),
        ));
    }
    let syntax =
        request::decode_document(&nodes).map_err(|error| request_syntax_problem(source, error))?;
    syntax
        .call
        .iter()
        .map(|syntax| decode_call(schema, source, syntax))
        .collect::<Result<Vec<_>, Mismatch>>()
        .map_err(|mismatch| problem(source, mismatch.span, mismatch.message))
}

fn request_syntax_problem(source: &str, error: request::DecodeError) -> Problem {
    let mut message = error.message();
    if error.found.contains(" AS ") {
        message = "found an annotation `AS` inside a value; expected one only around a whole argument of a call, as in `tag ([] AS: LIST Text)`; a value inside another is read by the type around it".into();
    }
    if let Some(intrinsic) = crate::prelude::renamed(&source[error.span.start..error.span.end]) {
        message.push_str(&format!(
            "; the built-in type `{}` is written in Title Case",
            intrinsic.name
        ));
    }
    problem(source, error.span, message)
}

/// Reads a flat method shape, choosing an overload by its argument types.
fn decode_call(schema: &Schema, source: &str, syntax: &request::Call) -> Result<Call, Mismatch> {
    let span = syntax.span;
    let call = syntax;
    let methods: Vec<usize> = schema.calls().collect();
    let all_methods = || {
        let mut names: Vec<String> = Vec::new();
        for method in &schema.methods {
            let name = format!("`{}`", method.full_name());
            if !names.contains(&name) {
                names.push(name);
            }
        }
        names.join(", ")
    };
    let text = &call.name.text;
    let candidates: Vec<usize> = methods
        .into_iter()
        .filter(|index| schema.method_at(*index).shape.text == *text)
        .collect();
    if candidates.is_empty() {
        return Err(Mismatch {
            span,
            message: format!(
                "found the call `{}`; expected one of the methods {}; a word in a slot, such as a choice's variant or `true`, goes in parentheses",
                text,
                all_methods()
            ),
        });
    }
    let methods: Vec<&Method> = candidates
        .iter()
        .map(|index| schema.method_at(*index))
        .collect();
    let (chosen, arguments) = decode_arguments(schema, source, &methods, span, call)?;
    Ok(Call {
        method: candidates[chosen],
        arguments,
    })
}

/// Reads `node` as a call of `method` alone, by its full name, with every
/// argument written in full, as an example of the method in its schema is.
/// `source` is the text `node` is read from.
pub(crate) fn decode_call_of(
    schema: &Schema,
    source: &str,
    method: &Method,
    node: &Node,
) -> Result<Vec<Value>, Mismatch> {
    let syntax = request::decode_call(node).map_err(syntax_mismatch)?;
    let call = &syntax;
    if call.name.text == method.shape.text {
        decode_arguments(schema, source, &[method], node.span, call).map(|(_, arguments)| arguments)
    } else {
        Err(Mismatch {
            span: node.span,
            message: format!(
                "found a call of `{}`; expected a call of `{}`, the method the example is under, written by its full name as a request writes it",
                call.name.text,
                method.full_name()
            ),
        })
    }
}

/// An argument of a call as written: its value, the type an annotation
/// claims for it, and the type it has of its own.
struct Argument<'n> {
    /// The whole argument, with its annotation.
    span: Span,
    value: &'n request::Value,
    annotated: Option<Type>,
    own: Option<Type>,
}

impl Argument<'_> {
    /// The type it states: its annotation's, else its own.
    fn stated(&self) -> Option<&Type> {
        self.annotated.as_ref().or(self.own.as_ref())
    }
}

/// Reads the arguments of `call`, a call of one of `methods`, all of the
/// same shape: which one it is a call of, and the arguments.
fn decode_arguments(
    schema: &Schema,
    source: &str,
    methods: &[&Method],
    span: Span,
    call: &request::Call,
) -> Result<(usize, Vec<Value>), Mismatch> {
    let text = &call.name.text;
    let arguments = call
        .arguments
        .iter()
        .map(|argument| read_argument(schema, source, argument))
        .collect::<Result<Vec<_>, _>>()?;
    let chosen = match methods {
        [_] => 0,
        _ => choose(schema, source, methods, span, call, &arguments)?,
    };
    let method = methods[chosen];
    let values: Vec<_> = method
        .shape
        .parameters
        .iter()
        .zip(&arguments)
        .enumerate()
        .map(|(index, (parameter, argument))| {
            let in_parameter = |mismatch: Mismatch| Mismatch {
                message: if crate::refinement::has_refinements(schema, &parameter.r#type) {
                    format!("in `{}` of `{}` (`{text}`): {}", parameter.name, &source[call.span.start..call.span.end], mismatch.message)
                } else { format!("in `{}` of `{text}`: {}", parameter.name, mismatch.message) },
                ..mismatch
            };
            if let Some(annotated) = argument
                .annotated
                .as_ref()
                .filter(|annotated| **annotated != parameter.r#type)
            {
                return Err(in_parameter(Mismatch {
                    span: argument.span,
                    message: format!(
                        "found `AS: {}`; expected `AS: {}`, the type of `{}`",
                        schema.type_name(annotated),
                        schema.type_name(&parameter.r#type),
                        parameter.name
                    ),
                }));
            }
            let value = crate::request::decode(schema, &parameter.r#type, argument.value)
                .map_err(in_parameter)?;
            if !argument.stated().is_some_and(|stated| parameter.r#type.accepts_argument_type(stated)) {
                let fix = fix(schema, source, method, call, index);
                let written = written(source, argument.value.span());
                let message = match &argument.own {
                    None => format!("`{written}` has no type of its own; annotate it, as in `{fix}`"),
                    Some(own) => format!(
                        "`{written}` has the type {} of its own, not `{}`; annotate it, as in `{fix}`",
                        schema.type_name(own),
                        schema.type_name(&parameter.r#type)
                    ),
                };
                return Err(in_parameter(Mismatch {
                    span: argument.span,
                    message,
                }));
            }
            Ok(value)
        })
        .collect::<Result<_, _>>()?;
    crate::refinement::check_precondition(schema, method, &values).map_err(|message| Mismatch {
        span,
        message: format!("in `{}`: {message}", written(source, span)),
    })?;
    Ok((chosen, values))
}

/// Reads an argument, `value` or `value AS: type`.
fn read_argument<'n>(
    schema: &Schema,
    source: &str,
    argument: &'n request::Argument,
) -> Result<Argument<'n>, Mismatch> {
    let (value, written_type) = match argument {
        request::Argument::Annotated { value, r#type, .. } => (value, Some(r#type)),
        request::Argument::Value(value) => (value, None),
    };
    let annotated = written_type.map(|written_type| {
        request_type(schema, written_type).ok_or_else(|| {
            let text = written(source, written_type.span());
            Mismatch {
                span: written_type.span(),
                message: format!("found `{text}` after `AS`, which names no type in this protocol; expected a plain type name such as `Text` or `File Id`, or a generic type such as `LIST Text`"),
            }
        })
    }).transpose()?;
    Ok(Argument {
        span: argument.span(),
        value,
        annotated,
        own: own_syntax_type(schema, value),
    })
}

/// Which of `methods`, overloads of one shape, a call is of: the one whose
/// parameters match the types its arguments state, allowing base types for
/// unannotated inline refinements. Predicates are checked after selection.
fn choose(
    schema: &Schema,
    source: &str,
    methods: &[&Method],
    span: Span,
    call: &request::Call,
    arguments: &[Argument],
) -> Result<usize, Mismatch> {
    let text = &call.name.text;
    let fits = |method: &Method, except: Option<usize>| {
        method
            .shape
            .parameters
            .iter()
            .zip(arguments)
            .enumerate()
            .all(|(index, (parameter, argument))| {
                Some(index) == except
                    || argument.stated().is_some_and(|stated| {
                        if argument.annotated.is_some() {
                            stated == &parameter.r#type
                        } else {
                            parameter.r#type.accepts_argument_type(stated)
                        }
                    })
            })
    };
    let fitting: Vec<usize> = (0..methods.len())
        .filter(|index| fits(methods[*index], None))
        .collect();
    if let [chosen] = fitting[..] {
        return Ok(chosen);
    }
    let overloads = |indices: &mut dyn Iterator<Item = usize>| {
        indices
            .map(|index| format!("`{}`", schema.typed_shape(&methods[index].shape)))
            .collect::<Vec<_>>()
            .join(", ")
    };
    let all = overloads(&mut (0..methods.len()));
    if !fitting.is_empty() {
        return Err(Mismatch {
            span,
            message: format!(
                "found `{text}` with arguments that fit several of its overloads, {}; expected them to fit one; annotate each argument with its parameter's type, as in `value AS: type`",
                overloads(&mut fitting.into_iter())
            ),
        });
    }
    let untyped = arguments
        .iter()
        .position(|argument| argument.stated().is_none());
    if let Some(index) = untyped {
        let example = methods
            .iter()
            .find(|method| fits(method, Some(index)))
            .unwrap_or(&methods[0]);
        return Err(Mismatch {
            span: arguments[index].span,
            message: format!(
                "in `{text}`: `{}` has no type of its own, so no overload of `{text}` can be chosen by it; annotate it with the type of the one to call, as in `{}`; the overloads are {all}",
                written(source, arguments[index].value.span()),
                fix(schema, source, example, call, index)
            ),
        });
    }
    let types: Vec<String> = arguments
        .iter()
        .filter_map(|argument| argument.stated())
        .map(|stated| format!("`{}`", schema.type_name(stated)))
        .collect();
    Err(Mismatch {
        span,
        message: format!(
            "found `{text}` with {} {}; expected the types of one of its overloads: {all}; a value of a named type, or of `Bytes` or `Instant`, is written like another type's, so annotate it, as in `(\"abc\" AS: File Id)`",
            match types.len() {
                1 => "an argument of the type",
                _ => "arguments of the types",
            },
            types.join(", ")
        ),
    })
}

/// A value as written in `source`, shortened when it is long: `[…]`.
fn written(source: &str, span: Span) -> String {
    let text = &source[span.start..span.end];
    if text.chars().count() <= 40 && !text.contains('\n') {
        return text.to_string();
    }
    match text.trim_start().chars().next() {
        Some('[') => "[…]".to_string(),
        Some('{') => "{…}".to_string(),
        Some('"') => "\"…\"".to_string(),
        _ => "…".to_string(),
    }
}

/// `call` as a call of `method`, with its argument at `index` annotated
/// with its parameter's type: `tag ([] AS: LIST Text)`.
fn fix(
    schema: &Schema,
    source: &str,
    method: &Method,
    call: &request::Call,
    index: usize,
) -> String {
    let mut slots = call.arguments.iter().enumerate().peekable();
    method
        .shape
        .text
        .split(' ')
        .map(|word| match (word, slots.next_if(|_| word == "_")) {
            (_, Some((slot, argument))) if slot == index => {
                let value = match argument {
                    request::Argument::Annotated { value, .. }
                    | request::Argument::Value(value) => value,
                };
                format!(
                    "({} AS: {})",
                    written(source, value.span()),
                    schema.type_name(&method.shape.parameters[slot].r#type)
                )
            }
            (_, Some((_, argument))) => match argument {
                request::Argument::Annotated { .. }
                | request::Argument::Value(
                    request::Value::Call(_)
                    | request::Value::Duration(_),
                ) => format!("({})", written(source, argument.span())),
                _ => written(source, argument.span()),
            },
            (word, None) => word.to_string(),
        })
        .collect::<Vec<_>>()
        .join(" ")
}

pub(crate) fn describe(node: &Node) -> String {
    match &node.kind {
        NodeKind::Number(text) => format!("the number `{text}`"),
        NodeKind::String(_) => "a string".to_string(),
        NodeKind::Call(call) => format!("the call `{}`", call.name_text()),
        NodeKind::Parens(_) => "parentheses".to_string(),
        NodeKind::Brackets(_) => "a list `[…]`".to_string(),
        NodeKind::Braces(_) => "braces `{…}`".to_string(),
        NodeKind::Underscore => "`_`".to_string(),
        NodeKind::Missing => "nothing".to_string(),
    }
}

/// Reads `node` as a value of `type`.
pub(crate) fn decode(schema: &Schema, r#type: &Type, node: &Node) -> Result<Value, Mismatch> {
    let syntax = request::decode_value(node).map_err(syntax_mismatch)?;
    crate::request::decode(schema, r#type, &syntax)
}

/// Reads `node` as an entry of `fields[index]`, as a field's `DEFAULT` is.
pub(crate) fn decode_entry(
    schema: &Schema,
    fields: &[Field],
    index: usize,
    node: &Node,
) -> Result<Value, Mismatch> {
    let syntax = request::decode_value(node).map_err(syntax_mismatch)?;
    crate::request::decode_field_entry(schema, fields, index, &syntax)
}

fn syntax_mismatch(error: request::DecodeError) -> Mismatch {
    Mismatch {
        span: error.span,
        message: error.message(),
    }
}

pub(crate) const INSTANT_EXAMPLE: &str = "2026-11-14T06:00:08.000Z";
pub(crate) const DURATION_EXAMPLE: &str = "1 hour 30 minutes";

/// Reads `node` as one of `shapes`, written as a variant: `missing`,
/// `conflict at revision 4`.
pub(crate) fn decode_variant(
    schema: &Schema,
    shapes: &[&Shape],
    node: &Node,
    what: &str,
) -> Result<Value, Mismatch> {
    let syntax = request::decode_value(node).map_err(syntax_mismatch)?;
    crate::request::variant(schema, shapes, &syntax, what)
}

/// Writes calls as a request, laid out the way `anyx::format` lays them out.
pub fn encode_request(schema: &Schema, calls: &[Call]) -> Result<String, EncodeError> {
    let written = calls
        .iter()
        .map(|call| {
            let method = schema.method_at(call.method);
            let mut shape = String::new();
            write_call(schema, method, &call.arguments, &mut shape)?;
            crate::refinement::check_precondition(schema, method, &call.arguments)
                .map_err(|message| EncodeError { message })?;
            Ok(layout(shape).trim_end().to_string())
        })
        .collect::<Result<Vec<_>, EncodeError>>()?;
    Ok(match written.as_slice() {
        [call] => format!("{call}\n"),
        calls => calls.iter().map(|call| format!("{call},\n")).collect(),
    })
}

/// Reads one literal argument, including an optional `AS` annotation, and
/// synthesizes its type independently of any enclosing method.
pub fn decode_argument(schema: &Schema, source: &str) -> Result<(Type, Value), Problem> {
    let node = single_node(source)?;
    let syntax =
        request::decode_argument(&node).map_err(|error| request_syntax_problem(source, error))?;
    let argument = read_argument(schema, source, &syntax)
        .map_err(|error| problem(source, error.span, error.message))?;
    let expected = argument.stated().ok_or_else(|| {
        problem(
            source,
            node.span,
            "this value has no unambiguous type of its own; annotate it with `AS: Type`".into(),
        )
    })?;
    let value = crate::request::decode(schema, expected, argument.value)
        .map_err(|error| problem(source, error.span, error.message))?;
    Ok((expected.clone(), value))
}

/// Reads a literal in a known type, such as a record field. Unlike a method
/// argument, it does not need to state that type itself.
pub fn decode_value(schema: &Schema, expected: &Type, source: &str) -> Result<Value, Problem> {
    let node = single_node(source)?;
    decode(schema, expected, &node).map_err(|error| problem(source, error.span, error.message))
}

/// Resolves a type expression in this protocol's namespace.
pub fn decode_type(schema: &Schema, source: &str) -> Result<Type, Problem> {
    let node = single_node(source)?;
    let syntax =
        request::decode_type(&node).map_err(|error| request_syntax_problem(source, error))?;
    request_type(schema, &syntax).ok_or_else(|| {
        problem(
            source,
            node.span,
            "expected a type declared in this protocol or its prelude".into(),
        )
    })
}

fn single_node(source: &str) -> Result<Node, Problem> {
    let mut nodes = parse(source)?;
    if nodes.len() != 1 {
        return Err(problem(
            source,
            Span::empty(0),
            "expected exactly one expression".into(),
        ));
    }
    Ok(nodes.remove(0))
}

/// Writes a standalone value, without a protocol response envelope.
/// The value is checked against its type before it is returned.
/// `Nothing` is not a value and cannot be written this way.
pub fn encode_value(
    schema: &Schema,
    expected: &Type,
    value: &Value,
) -> Result<String, EncodeError> {
    let mut text = String::new();
    write(schema, expected, value, &mut text)?;
    let nodes = parse(&text).map_err(|problem| EncodeError {
        message: problem.message,
    })?;
    let [node] = nodes.as_slice() else {
        return Err(EncodeError {
            message: "expected exactly one value".into(),
        });
    };
    decode(schema, expected, node).map_err(|mismatch| EncodeError {
        message: mismatch.message,
    })?;
    Ok(layout(text))
}

/// Lays `text` out canonically. The text is always valid anyx, so the
/// formatter never fails on it; if it did, the text is still a fine message.
pub(crate) fn layout(text: String) -> String {
    anyx::format(&text).unwrap_or(text)
}

/// Writes a call of `method` in a request: its words, with `arguments` in
/// its slots, each annotated with its parameter's type, `([] AS: LIST:
/// Text)`, unless it has that type of its own as written.
fn write_call(
    schema: &Schema,
    method: &Method,
    arguments: &[Value],
    out: &mut String,
) -> Result<(), EncodeError> {
    let shape = &method.shape;
    check_count(shape, arguments)?;
    let mut slots = shape.parameters.iter().zip(arguments).peekable();
    for (index, word) in shape.text.split(' ').enumerate() {
        if index > 0 {
            out.push(' ');
        }
        let Some((parameter, value)) = slots.next_if(|_| word == "_") else {
            out.push_str(word);
            continue;
        };
        let mut written = String::new();
        write(schema, &parameter.r#type, value, &mut written)?;
        let own = match &parse(&written).as_deref() {
            Ok([node]) => own_type(schema, node),
            _ => None,
        };
        match own.as_ref() == Some(&parameter.r#type) {
            true => write_slot(schema, &parameter.r#type, value, out)?,
            false => out.push_str(&format!(
                "({written} AS: {})",
                schema.type_name(&parameter.r#type)
            )),
        }
    }
    Ok(())
}

fn check_count(shape: &Shape, arguments: &[Value]) -> Result<(), EncodeError> {
    match arguments.len() == shape.parameters.len() {
        true => Ok(()),
        false => Err(EncodeError {
            message: format!(
                "found {} arguments for `{}`; expected {}",
                arguments.len(),
                shape.text,
                shape.parameters.len()
            ),
        }),
    }
}

/// Writes a call of `shape`: its words, with `arguments` in its slots.
pub(crate) fn write_shape(
    schema: &Schema,
    shape: &Shape,
    arguments: &[Value],
    out: &mut String,
) -> Result<(), EncodeError> {
    check_count(shape, arguments)?;
    let mut slots = shape.parameters.iter().zip(arguments).peekable();
    for (index, word) in shape.text.split(' ').enumerate() {
        if index > 0 {
            out.push(' ');
        }
        match (word, slots.next_if(|_| word == "_")) {
            (_, Some((parameter, value))) => write_slot(schema, &parameter.r#type, value, out)?,
            _ => out.push_str(word),
        }
    }
    Ok(())
}

/// Writes a value that fills a slot: a word there would join the call's
/// name, so a value written as a call goes in parentheses.
fn write_slot(
    schema: &Schema,
    r#type: &Type,
    value: &Value,
    out: &mut String,
) -> Result<(), EncodeError> {
    let call = matches!(value, Value::Duration(_) | Value::Variant { .. });
    if call {
        out.push('(');
    }
    write(schema, r#type, value, out)?;
    if call {
        out.push(')');
    }
    Ok(())
}

pub(crate) fn write(
    schema: &Schema,
    r#type: &Type,
    value: &Value,
    out: &mut String,
) -> Result<(), EncodeError> {
    crate::refinement::validate(schema, r#type, value)
        .map_err(|message| EncodeError { message })?;
    let r#type = schema.underlying(r#type);
    if let Type::Union(_) = r#type {
        return match schema.member_holding(r#type, value) {
            Some(member) => write(schema, member, value, out),
            None => Err(mismatched(schema, r#type, &format!("{value:?}"))),
        };
    }
    match value {
        Value::Text(text) => write_text(text, out),
        Value::Bytes(bytes) => write_text(&crate::bytes::encode(bytes), out),
        Value::Integer(integer) => out.push_str(&integer.to_string()),
        Value::Float(number) if number.is_finite() => out.push_str(&format!("{number:?}")),
        Value::Float(number) => {
            return Err(EncodeError {
                message: format!("found the Float {number}; expected a finite one"),
            });
        }
        Value::Instant(instant) => match crate::time::format_instant(*instant) {
            Some(text) => write_text(&text, out),
            None => {
                return Err(EncodeError {
                    message: format!(
                        "found the instant {instant:?}; expected one in the years 0000 to 9999, which RFC 3339 writes"
                    ),
                });
            }
        },
        Value::Duration(duration) => match crate::time::format_duration(*duration) {
            Some(text) => out.push_str(&text),
            None => {
                return Err(EncodeError {
                    message: format!(
                        "found the duration {duration:?}; expected one whose milliseconds fit in 64 bits"
                    ),
                });
            }
        },
        Value::Nothing => {
            return Err(EncodeError {
                message: "found nothing where a value goes; expected a value, since only a method that `RETURNS NOTHING` returns nothing".to_string(),
            });
        }
        Value::List(items) => {
            let item = match r#type {
                Type::List(item) => item.as_ref(),
                _ => r#type,
            };
            out.push('[');
            for (index, value) in items.iter().enumerate() {
                if index > 0 {
                    out.push_str(", ");
                }
                write(schema, item, value, out)?;
            }
            out.push(']');
        }
        Value::Tuple(_) => {
            return Err(EncodeError {
                message:
                    "found a field's slots where a value goes; expected them in the field's shape"
                        .to_string(),
            });
        }
        Value::Map(entries) => {
            let Type::Map(value_type) = r#type else {
                return Err(mismatched(schema, r#type, "a map"));
            };
            if entries.is_empty() {
                out.push_str("{}");
                return Ok(());
            }
            out.push_str("{ ");
            for (index, (key, value)) in entries.iter().enumerate() {
                if index > 0 {
                    out.push_str(", ");
                }
                write_quoted(key, out);
                out.push_str(": ");
                write(schema, value_type, value, out)?;
            }
            out.push_str(" }");
        }
        Value::Record(values) => {
            let Some((_, kind)) = schema.instance(r#type) else {
                return Err(mismatched(schema, r#type, "a record"));
            };
            let TypeKind::Record(fields) = kind.as_ref() else {
                return Err(mismatched(schema, r#type, "a record"));
            };
            write_fields(schema, fields, values, out)?;
        }
        Value::Variant { shape, arguments } => {
            if let Type::Variant(own) = r#type
                && own.text == *shape
            {
                return write_shape(schema, own, arguments, out);
            }
            let Some((_, kind)) = schema.instance(r#type) else {
                return Err(mismatched(schema, r#type, "a variant"));
            };
            let TypeKind::Choice(variants) = kind.as_ref() else {
                return Err(mismatched(schema, r#type, "a variant"));
            };
            let Some(variant) = variants.iter().find(|variant| variant.shape.text == *shape) else {
                return Err(EncodeError {
                    message: format!(
                        "found the variant `{shape}`; expected a variant of `{}`",
                        schema.type_name(r#type)
                    ),
                });
            };
            write_shape(schema, &variant.shape, arguments, out)?;
        }
    }
    Ok(())
}

fn mismatched(schema: &Schema, r#type: &Type, found: &str) -> EncodeError {
    EncodeError {
        message: format!("found {found}; expected {}", schema.type_name(r#type)),
    }
}

/// Writes fields in the schema's order, each as a reader writes it: a
/// shape whose slot is last as its words, a `:`, and its value; another
/// shape with its value in the slot; a checkbox as its words when `true`;
/// an `@` field as `@` and its value. The layout drops each `:` that a
/// reader doesn't need.
fn write_fields(
    schema: &Schema,
    fields: &[Field],
    values: &BTreeMap<String, Value>,
    out: &mut String,
) -> Result<(), EncodeError> {
    let mut written = Vec::new();
    for field in fields {
        let Some(value) = values.get(&field.name) else {
            continue;
        };
        // A checkbox, an `@` field and a `null` read best left out; any
        // other default is written, so a reader sees it.
        let null = matches!(value, Value::Variant { shape, .. } if shape == "null")
            && schema.nullable_item(&field.r#type).is_some();
        if field.default.as_ref() == Some(value)
            && (null || matches!(field.form, FieldForm::Checkbox | FieldForm::Unnamed))
        {
            continue;
        }
        let mut entry = String::new();
        // Each slot's type and value.
        let slots: Vec<(&Type, &Value)> = match (&field.r#type, value) {
            (Type::Tuple(types), Value::Tuple(values)) if types.len() == values.len() => {
                types.iter().zip(values).collect()
            }
            (Type::Tuple(_), _) => {
                return Err(EncodeError {
                    message: format!(
                        "found {value:?} for the field `{}`; expected a value for each of its slots",
                        field.name
                    ),
                });
            }
            (r#type, value) => vec![(r#type, value)],
        };
        match &field.form {
            FieldForm::Checkbox => match value {
                Value::Variant { shape, .. } if shape == "true" => entry.push_str(&field.name),
                _ => continue,
            },
            // `path: "a"`: a slot last takes its value after a `:`.
            FieldForm::Shape(shape)
                if shape.ends_with(" _") && !shape[..shape.len() - 2].contains('_') =>
            {
                let [(r#type, item)] = slots[..] else {
                    unreachable!("a shape with one slot holds one value")
                };
                entry.push_str(&shape[..shape.len() - 2]);
                entry.push_str(": ");
                write(schema, r#type, item, &mut entry)?;
            }
            FieldForm::Shape(shape) => {
                let mut slots = slots.iter();
                for (index, word) in shape.split(' ').enumerate() {
                    if index > 0 {
                        entry.push(' ');
                    }
                    match (word, if word == "_" { slots.next() } else { None }) {
                        (_, Some((r#type, item))) => write_slot(schema, r#type, item, &mut entry)?,
                        (word, None) => entry.push_str(word),
                    }
                }
            }
            FieldForm::Unnamed => {
                let [(r#type, item)] = slots[..] else {
                    unreachable!("an `@` field holds one value")
                };
                let mut value = String::new();
                write(schema, r#type, item, &mut value)?;
                entry.push('@');
                // `@-3` would read as the symbol word `@-`.
                if value.starts_with(|c: char| !c.is_alphanumeric() && !"\"([{".contains(c)) {
                    entry.push(' ');
                }
                entry.push_str(&value);
            }
        }
        written.push(entry);
    }
    if written.is_empty() {
        out.push_str("{}");
    } else {
        out.push_str("{ ");
        out.push_str(&written.join(", "));
        out.push_str(" }");
    }
    Ok(())
}

/// Writes text as a raw string when it has lines and reads back the same,
/// and as a quoted string otherwise.
pub(crate) fn write_text(text: &str, out: &mut String) {
    if text.contains('\n')
        && let Some(raw) = raw_string(text)
    {
        out.push_str(&raw);
        return;
    }
    write_quoted(text, out);
}

/// Writes `text` as a quoted string.
fn write_quoted(text: &str, out: &mut String) {
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&format!("\\u{{{:x}}}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

/// `text` as a raw string, with more quotes than any line starts with, or
/// `None` when a raw string can't hold it exactly, such as a line of only
/// spaces.
fn raw_string(text: &str) -> Option<String> {
    let longest = text
        .lines()
        .map(|line| {
            line.trim_start_matches([' ', '\t'])
                .chars()
                .take_while(|&c| c == '"')
                .count()
        })
        .max()
        .unwrap_or(0);
    let quotes = "\"".repeat((longest + 1).max(3));
    let raw = format!("{quotes}\n{text}\n{quotes}");
    let parsed = anyx::parse(&raw);
    match &parsed.syntax.nodes()[..] {
        [
            Node {
                kind: NodeKind::String(literal),
                ..
            },
        ] if parsed.errors.is_empty() && literal.value == text => Some(raw),
        _ => None,
    }
}
