//! Reads and writes requests, and the values in them and in responses
//! (`response`), checking each value against the schema.
//!
//! A request is an anyx document of calls, each written in its method's
//! shape: `read "Hymns/a.poentaro"`.

use std::collections::BTreeMap;

use anyx::{NamePart, Node, NodeKind, Span, line_column};

use crate::annotation::{annotated_type, annotation, own_type};
use crate::schema::{Field, Method, Schema, Shape, Type, TypeKind};
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

fn quoted_shapes<'a>(shapes: impl Iterator<Item = &'a Shape>) -> String {
    shapes
        .map(|shape| format!("`{}`", shape.text))
        .collect::<Vec<_>>()
        .join(", ")
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
    nodes
        .iter()
        .map(|node| decode_call(schema, source, node))
        .collect::<Result<Vec<_>, Mismatch>>()
        .map_err(|mismatch| problem(source, mismatch.span, mismatch.message))
}

/// Reads a flat method shape, choosing an overload by its argument types.
fn decode_call(schema: &Schema, source: &str, node: &Node) -> Result<Call, Mismatch> {
    let call = node;
    let methods: Vec<usize> = (0..=schema.introspection()).collect();
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
    let NodeKind::Call(found) = &call.kind else {
        return Err(Mismatch {
            span: node.span,
            message: format!(
                "found {}; expected one of the methods {}; a word in a slot, such as a choice's variant or `true`, goes in parentheses",
                describe(call),
                all_methods()
            ),
        });
    };
    let text = found.name_text();
    let candidates: Vec<usize> = methods
        .into_iter()
        .filter(|index| schema.method_at(*index).shape.text == text)
        .collect();
    if candidates.is_empty() {
        return Err(Mismatch {
            span: node.span,
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
    let (chosen, arguments) = decode_arguments(schema, source, &methods, node, found)?;
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
    let call = node;
    let found = match &call.kind {
        NodeKind::Call(found) => format!("a call of `{}`", found.name_text()),
        _ => describe(call),
    };
    let expected = method.full_name();
    let mismatch = |found: String| Mismatch {
        span: node.span,
        message: format!(
            "found {found}; expected a call of `{expected}`, the method the example is under, written by its full name as a request writes it"
        ),
    };
    match &call.kind {
        NodeKind::Call(call) if call.name_text() == method.shape.text => {
            decode_arguments(schema, source, &[method], node, call).map(|(_, arguments)| arguments)
        }
        _ => Err(mismatch(found)),
    }
}

/// An argument of a call as written: its value, the type an annotation
/// claims for it, and the type it has of its own.
struct Argument<'n> {
    /// The whole argument, with its annotation.
    node: &'n Node,
    value: &'n Node,
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
    node: &Node,
    call: &anyx::Call,
) -> Result<(usize, Vec<Value>), Mismatch> {
    let text = call.name_text();
    let arguments = call
        .arguments
        .iter()
        .map(|argument| read_argument(schema, source, argument))
        .collect::<Result<Vec<_>, _>>()?;
    let chosen = match methods {
        [_] => 0,
        _ => choose(schema, source, methods, node, call, &arguments)?,
    };
    let method = methods[chosen];
    let values = method
        .shape
        .parameters
        .iter()
        .zip(&arguments)
        .enumerate()
        .map(|(index, (parameter, argument))| {
            let in_parameter = |mismatch: Mismatch| Mismatch {
                message: format!("in `{}` of `{text}`: {}", parameter.name, mismatch.message),
                ..mismatch
            };
            if let Some(annotated) = argument
                .annotated
                .as_ref()
                .filter(|annotated| **annotated != parameter.r#type)
            {
                return Err(in_parameter(Mismatch {
                    span: argument.node.span,
                    message: format!(
                        "found `AS: {}`; expected `AS: {}`, the type of `{}`",
                        schema.type_name(annotated),
                        schema.type_name(&parameter.r#type),
                        parameter.name
                    ),
                }));
            }
            let value = decode(schema, &parameter.r#type, argument.value).map_err(in_parameter)?;
            if argument.stated() != Some(&parameter.r#type) {
                let fix = fix(schema, source, method, call, index);
                let written = written(source, argument.value);
                let message = match &argument.own {
                    None => format!("`{written}` has no type of its own; annotate it, as in `{fix}`"),
                    Some(own) => format!(
                        "`{written}` has the type {} of its own, not `{}`; annotate it, as in `{fix}`",
                        schema.type_name(own),
                        schema.type_name(&parameter.r#type)
                    ),
                };
                return Err(in_parameter(Mismatch {
                    span: argument.node.span,
                    message,
                }));
            }
            Ok(value)
        })
        .collect::<Result<_, _>>()?;
    Ok((chosen, values))
}

/// Reads an argument, `value` or `value AS: type`.
fn read_argument<'n>(
    schema: &Schema,
    source: &str,
    node: &'n Node,
) -> Result<Argument<'n>, Mismatch> {
    let (value, written_type) = match annotation(node) {
        Some((value, written_type)) => (value, Some(written_type)),
        None => (node, None),
    };
    if let Some(inner) = inner_annotation(value) {
        return Err(Mismatch {
            span: inner.span,
            message: "found an annotation `AS` inside a value; expected one only around a whole argument of a call, as in `tag ([] AS: LIST Text)`; a value inside another is read by the type around it".to_string(),
        });
    }
    let Some(written_type) = written_type else {
        return Ok(Argument {
            node,
            value: node,
            annotated: None,
            own: own_type(schema, node),
        });
    };
    let annotated = annotated_type(schema, written_type).ok_or_else(|| {
        let text = written(source, written_type);
        let renamed = text
            .split(|c: char| !c.is_alphanumeric())
            .find_map(crate::prelude::renamed)
            .map_or(String::new(), |intrinsic| {
                format!(
                    "; the built-in type `{}` is written in Title Case",
                    intrinsic.name
                )
            });
        Mismatch {
            span: written_type.span,
            message: format!(
                "found `{text}` after `AS`, which names no type in this protocol; expected a plain type name such as `Text` or `File Id`, or a generic type such as `LIST Text`{renamed}"
            ),
        }
    })?;
    Ok(Argument {
        node,
        value,
        annotated: Some(annotated),
        own: own_type(schema, value),
    })
}

/// The first annotation inside `node`, or `node` itself if it is one.
fn inner_annotation(node: &Node) -> Option<&Node> {
    if annotation(node).is_some() {
        return Some(node);
    }
    match &node.kind {
        NodeKind::Call(call) => call.arguments.iter().find_map(inner_annotation),
        NodeKind::Parens(nodes) | NodeKind::Brackets(nodes) | NodeKind::Braces(nodes) => {
            nodes.iter().find_map(inner_annotation)
        }
        _ => None,
    }
}

/// Which of `methods`, overloads of one shape, a call is of: the one whose
/// parameters have exactly the types its arguments state.
fn choose(
    schema: &Schema,
    source: &str,
    methods: &[&Method],
    node: &Node,
    call: &anyx::Call,
    arguments: &[Argument],
) -> Result<usize, Mismatch> {
    let text = call.name_text();
    let fits = |method: &Method, except: Option<usize>| {
        method
            .shape
            .parameters
            .iter()
            .zip(arguments)
            .enumerate()
            .all(|(index, (parameter, argument))| {
                Some(index) == except || argument.stated() == Some(&parameter.r#type)
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
            span: node.span,
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
            span: arguments[index].node.span,
            message: format!(
                "in `{text}`: `{}` has no type of its own, so no overload of `{text}` can be chosen by it; annotate it with the type of the one to call, as in `{}`; the overloads are {all}",
                written(source, arguments[index].value),
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
        span: node.span,
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
fn written(source: &str, node: &Node) -> String {
    let text = &source[node.span.start..node.span.end];
    if text.chars().count() <= 40 && !text.contains('\n') {
        return text.to_string();
    }
    match &node.kind {
        NodeKind::Brackets(_) => "[…]".to_string(),
        NodeKind::Braces(_) => "{…}".to_string(),
        NodeKind::String(_) => "\"…\"".to_string(),
        _ => "…".to_string(),
    }
}

/// `call` as a call of `method`, with its argument at `index` annotated
/// with its parameter's type: `tag ([] AS: LIST Text)`.
fn fix(schema: &Schema, source: &str, method: &Method, call: &anyx::Call, index: usize) -> String {
    let mut slots = call.arguments.iter().enumerate().peekable();
    method
        .shape
        .text
        .split(' ')
        .map(|word| match (word, slots.next_if(|_| word == "_")) {
            (_, Some((slot, argument))) if slot == index => {
                let value = annotation(argument).map_or(argument, |(value, _)| value);
                format!(
                    "({} AS: {})",
                    written(source, value),
                    schema.type_name(&method.shape.parameters[slot].r#type)
                )
            }
            (_, Some((_, argument))) => match &argument.kind {
                NodeKind::Call(_) => format!("({})", written(source, argument)),
                _ => written(source, argument),
            },
            (word, None) => word.to_string(),
        })
        .collect::<Vec<_>>()
        .join(" ")
}

enum ShapeMismatch {
    /// The node is no call of any of the shapes; holds what it is.
    NoShape(String),
    Argument(Mismatch),
}

/// Reads `node` as a call of one of `shapes`: which one, and its arguments.
fn decode_shape(
    schema: &Schema,
    shapes: &[&Shape],
    node: &Node,
) -> Result<(usize, Vec<Value>), ShapeMismatch> {
    let NodeKind::Call(call) = &node.kind else {
        return Err(ShapeMismatch::NoShape(describe(node)));
    };
    let text = call.name_text();
    let Some(index) = shapes.iter().position(|shape| shape.text == text) else {
        return Err(ShapeMismatch::NoShape(format!("the call `{text}`")));
    };
    let arguments = shapes[index]
        .parameters
        .iter()
        .zip(&call.arguments)
        .map(|(parameter, argument)| {
            decode(schema, &parameter.r#type, argument).map_err(|mismatch| Mismatch {
                message: format!("in `{}` of `{text}`: {}", parameter.name, mismatch.message),
                ..mismatch
            })
        })
        .collect::<Result<_, _>>()
        .map_err(ShapeMismatch::Argument)?;
    Ok((index, arguments))
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
    if annotation(node).is_some() {
        return Err(Mismatch {
            span: node.span,
            message: "found an annotation `AS` inside a value; expected one only around a whole argument of a call, as in `tag ([] AS: LIST Text)`; a value inside another is read by the type around it".to_string(),
        });
    }
    let r#type = schema.underlying(r#type);
    let mismatch = |expected: String| Mismatch {
        span: node.span,
        message: format!("found {}; expected {expected}", describe(node)),
    };
    match (r#type, &node.kind) {
        (Type::Text, NodeKind::String(literal)) => Ok(Value::Text(literal.value.clone())),
        (Type::Text, _) => Err(mismatch("a string, as in `\"text\"`".to_string())),
        (Type::Bytes, NodeKind::String(literal)) => crate::bytes::decode(&literal.value)
            .map(Value::Bytes)
            .ok_or_else(|| mismatch("base64 in the string, as in `\"TVRoZA==\"`".to_string())),
        (Type::Bytes, _) => Err(mismatch(
            "base64 in a string, as in `\"TVRoZA==\"`".to_string(),
        )),
        (Type::Integer, NodeKind::Number(text)) if is_float_text(text) => Err(Mismatch {
            span: node.span,
            message: format!(
                "found the Float `{text}`; expected an Integer, a whole number written without a point, such as `3`"
            ),
        }),
        (Type::Integer, NodeKind::Number(text)) => text
            .parse()
            .map(Value::Integer)
            .map_err(|_| mismatch("a whole number that fits in 64 bits".to_string())),
        (Type::Integer, _) => Err(mismatch("a whole number, as in `3`".to_string())),
        (Type::Float, NodeKind::Number(text)) if !is_float_text(text) => Err(Mismatch {
            span: node.span,
            message: format!("found the Integer `{text}`; expected a Float, such as `{text}.0`"),
        }),
        (Type::Float, NodeKind::Number(text)) => text
            .parse()
            .ok()
            .filter(|number: &f64| number.is_finite())
            .map(Value::Float)
            .ok_or_else(|| mismatch("a finite Float, as in `1.5`".to_string())),
        (Type::Float, _) => Err(mismatch("a Float, as in `1.5`".to_string())),
        (Type::Boolean, NodeKind::Call(call)) if call.arguments.is_empty() => {
            match call.name_text().as_str() {
                "true" => Ok(Value::Boolean(true)),
                "false" => Ok(Value::Boolean(false)),
                _ => Err(mismatch("`true` or `false`".to_string())),
            }
        }
        (Type::Boolean, _) => Err(mismatch("`true` or `false`".to_string())),
        (Type::Instant, NodeKind::String(literal)) => crate::time::parse_instant(&literal.value)
            .map(Value::Instant)
            .ok_or_else(|| {
                mismatch(format!(
                    "an instant in UTC in the string, as in `\"{INSTANT_EXAMPLE}\"`"
                ))
            }),
        (Type::Instant, _) => Err(mismatch(format!(
            "an instant in UTC in a string, as in `\"{INSTANT_EXAMPLE}\"`"
        ))),
        (Type::Duration, NodeKind::Call(call)) => {
            let pairs = duration_pairs(call).ok_or_else(|| {
                mismatch(format!(
                    "a duration, whole numbers each with a unit, as in `{DURATION_EXAMPLE}`"
                ))
            })?;
            crate::time::parse_duration(&pairs)
                .map(Value::Duration)
                .map_err(|found| Mismatch {
                    span: node.span,
                    message: format!(
                        "found {found}; expected a duration, whole numbers each with a unit, the largest first, as in `{DURATION_EXAMPLE}`; the units are {}",
                        duration_units()
                    ),
                })
        }
        (Type::Duration, _) => Err(mismatch(format!(
            "a duration, as in `{DURATION_EXAMPLE}`, which goes in parentheses in a slot"
        ))),
        (Type::List(item), NodeKind::Brackets(items)) => items
            .iter()
            .map(|node| decode(schema, item, node))
            .collect::<Result<_, _>>()
            .map(Value::List),
        (Type::List(item), _) => Err(mismatch(format!(
            "a list of {}, as in `[…]`",
            schema.type_name(item)
        ))),
        (Type::Map(value), NodeKind::Braces(nodes)) => {
            decode_entries(schema, value, nodes).map(Value::Map)
        }
        (Type::Map(value), _) => Err(mismatch(format!(
            "a map of {}, as in `{{ \"key\" … }}`",
            schema.type_name(value)
        ))),
        (Type::Named(_) | Type::Generic(..), _) => {
            let Some((definition, kind)) = schema.instance(r#type) else {
                unreachable!("a named or generic type is a record or a choice underneath")
            };
            match kind.as_ref() {
                TypeKind::Record(fields) => match &node.kind {
                    NodeKind::Braces(nodes) => {
                        decode_fields(schema, fields, nodes, node.span).map(Value::Record)
                    }
                    _ => Err(mismatch(format!(
                        "the record `{}`, as in `{{ {} }}`",
                        definition.shape(),
                        example_fields(fields)
                    ))),
                },
                TypeKind::Choice(variants) => {
                    let shapes: Vec<&Shape> =
                        variants.iter().map(|variant| &variant.shape).collect();
                    decode_variant(
                        schema,
                        &shapes,
                        node,
                        &format!("a variant of `{}`", definition.shape()),
                    )
                }
                TypeKind::Distinct(_) => unreachable!("`underlying` reads past named types"),
            }
        }
        // The checker replaces each parameter by its argument wherever a
        // value is read.
        (Type::Parameter(name), _) => Err(mismatch(format!(
            "a value of a type in place of the type parameter `{name}`"
        ))),
    }
}

/// Whether a number is written as a `Float`, with a point or an exponent:
/// `1.5`, `3.0`, `1e3`. Any other number, such as `3` or `-1`, is an
/// `Integer`, so every number literal states its own type.
pub(crate) fn is_float_text(text: &str) -> bool {
    let digits = text.strip_prefix('-').unwrap_or(text);
    let based = ["0x", "0X", "0b", "0B", "0o", "0O"]
        .iter()
        .any(|prefix| digits.starts_with(prefix));
    !based && digits.contains(['.', 'e', 'E'])
}

pub(crate) const INSTANT_EXAMPLE: &str = "2026-11-14T06:00:08.000Z";
pub(crate) const DURATION_EXAMPLE: &str = "1 hour 30 minutes";

/// The units of a duration, as a message lists them.
fn duration_units() -> String {
    crate::time::units()
        .iter()
        .map(|unit| format!("`{}`", unit.plural))
        .collect::<Vec<_>>()
        .join(", ")
}

/// A duration's numbers and units, `[("1", "hour"), ("30", "minutes")]`,
/// if `call` alternates a number and a word: `1 hour 30 minutes` is the
/// call `_ hour _ minutes` of `1` and `30`.
pub(crate) fn duration_pairs(call: &anyx::Call) -> Option<Vec<(&str, &str)>> {
    if call.name.len() != call.arguments.len() * 2 {
        return None;
    }
    call.name
        .chunks(2)
        .zip(&call.arguments)
        .map(|(parts, argument)| match (parts, &argument.kind) {
            ([NamePart::Slot, NamePart::Word { text, .. }], NodeKind::Number(count)) => {
                Some((count.as_str(), text.as_str()))
            }
            _ => None,
        })
        .collect()
}

/// Reads `node` as one of `shapes`, written as a variant: `missing`,
/// `conflict at revision 4`.
pub(crate) fn decode_variant(
    schema: &Schema,
    shapes: &[&Shape],
    node: &Node,
    what: &str,
) -> Result<Value, Mismatch> {
    match decode_shape(schema, shapes, node) {
        Ok((index, arguments)) => Ok(Value::Variant {
            shape: shapes[index].text.clone(),
            arguments,
        }),
        Err(ShapeMismatch::NoShape(found)) => Err(Mismatch {
            span: node.span,
            message: format!(
                "found {found}; expected {what}: {}",
                quoted_shapes(shapes.iter().copied())
            ),
        }),
        Err(ShapeMismatch::Argument(mismatch)) => Err(mismatch),
    }
}

fn example_fields(fields: &[Field]) -> String {
    fields
        .iter()
        .map(|field| format!("{} …", field.name))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Reads a map's entries, each a string key and its value: `"hymn" 3`, or
/// `"hymn": done` when the value is words.
fn decode_entries(
    schema: &Schema,
    r#type: &Type,
    nodes: &[Node],
) -> Result<BTreeMap<String, Value>, Mismatch> {
    let mut entries = BTreeMap::new();
    for node in nodes {
        let entry = match &node.kind {
            NodeKind::Call(call) => match (&call.name[..], &call.arguments[..]) {
                ([NamePart::Slot, NamePart::Slot], [key, value]) => match &key.kind {
                    NodeKind::String(key) => Some((&key.value, value)),
                    _ => None,
                },
                _ => None,
            },
            _ => None,
        };
        let Some((key, value)) = entry else {
            let found = match &node.kind {
                NodeKind::Call(call) => format!("the call `{}`", call.name_text()),
                _ => describe(node),
            };
            return Err(Mismatch {
                span: node.span,
                message: format!(
                    "found {found}; expected an entry written as a string key and its value, as in `\"key\" …`; put a `:` after the key when the value is words, as in `\"key\": done`"
                ),
            });
        };
        if entries.contains_key(key) {
            return Err(Mismatch {
                span: node.span,
                message: format!("found the key `\"{key}\"` twice; expected each key once"),
            });
        }
        let value = decode(schema, r#type, value).map_err(|mismatch| Mismatch {
            message: format!("in `\"{key}\"`: {}", mismatch.message),
            ..mismatch
        })?;
        entries.insert(key.clone(), value);
    }
    Ok(entries)
}

fn decode_fields(
    schema: &Schema,
    fields: &[Field],
    nodes: &[Node],
    span: Span,
) -> Result<BTreeMap<String, Value>, Mismatch> {
    let mut values = BTreeMap::new();
    let names = || {
        fields
            .iter()
            .map(|field| format!("`{}`", field.name))
            .collect::<Vec<_>>()
            .join(", ")
    };
    for node in nodes {
        let NodeKind::Call(call) = &node.kind else {
            return Err(Mismatch {
                span: node.span,
                message: format!(
                    "found {}; expected a field written as its name and value, as in `{} …`",
                    describe(node),
                    fields.first().map_or("name", |field| field.name.as_str())
                ),
            });
        };
        let name = call.name_text();
        let (Some(words), [value]) = (name.strip_suffix(" _"), &call.arguments[..]) else {
            return Err(Mismatch {
                span: node.span,
                message: format!(
                    "found the call `{name}`; expected a field written as its name and value, as in `{} …`; put a `:` after the name when the value is a word, as in `status: done`",
                    fields.first().map_or("name", |field| field.name.as_str())
                ),
            });
        };
        let Some(field) = fields.iter().find(|field| field.name == words) else {
            return Err(Mismatch {
                span: node.span,
                message: format!("found the field `{words}`; expected one of {}", names()),
            });
        };
        if values.contains_key(words) {
            return Err(Mismatch {
                span: node.span,
                message: format!("found the field `{words}` twice; expected each field once"),
            });
        }
        let value = decode(schema, &field.r#type, value).map_err(|mismatch| Mismatch {
            message: format!("in `{words}`: {}", mismatch.message),
            ..mismatch
        })?;
        values.insert(words.to_string(), value);
    }
    for field in fields {
        if let (Some(default), false) = (&field.default, values.contains_key(&field.name)) {
            values.insert(field.name.clone(), default.clone());
        }
    }
    let missing: Vec<String> = fields
        .iter()
        .filter(|field| !field.optional && !values.contains_key(&field.name))
        .map(|field| format!("`{}`", field.name))
        .collect();
    if !missing.is_empty() {
        return Err(Mismatch {
            span,
            message: format!(
                "found no {}; expected every field that is not `OPTIONAL`",
                missing.join(", ")
            ),
        });
    }
    Ok(values)
}

/// Writes calls as a request, each call laid out the way `anyx::format`
/// lays it out in its method shape: `export midi "a"`. The
/// formatter would space the `/`s, which reads less like a name.
pub fn encode_request(schema: &Schema, calls: &[Call]) -> Result<String, EncodeError> {
    let written = calls
        .iter()
        .map(|call| {
            let method = schema.method_at(call.method);
            let mut shape = String::new();
            write_call(schema, method, &call.arguments, &mut shape)?;
            Ok(layout(shape).trim_end().to_string())
        })
        .collect::<Result<Vec<_>, EncodeError>>()?;
    Ok(match written.as_slice() {
        [call] => format!("{call}\n"),
        calls => calls.iter().map(|call| format!("{call},\n")).collect(),
    })
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
    let call = matches!(
        value,
        Value::Boolean(_) | Value::Duration(_) | Value::Variant { .. }
    );
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
    let r#type = schema.underlying(r#type);
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
        Value::Boolean(boolean) => out.push_str(if *boolean { "true" } else { "false" }),
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

/// Writes fields in the schema's order, each as its name, a `:`, and its
/// value; the layout drops each `:` that a reader doesn't need.
fn write_fields(
    schema: &Schema,
    fields: &[Field],
    values: &BTreeMap<String, Value>,
    out: &mut String,
) -> Result<(), EncodeError> {
    out.push_str("{ ");
    let mut first = true;
    for field in fields {
        let Some(value) = values.get(&field.name) else {
            continue;
        };
        if !first {
            out.push_str(", ");
        }
        first = false;
        out.push_str(&field.name);
        out.push_str(": ");
        write(schema, &field.r#type, value, out)?;
    }
    out.push_str(" }");
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
