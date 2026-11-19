//! The types of a request's arguments: the type a value has of its own,
//! from how it is written, and the type an annotation, `value AS: type`,
//! claims for it. A call resolves to the overload of its shape whose
//! parameters have exactly those types.

use anyx::{NamePart, Node};

use crate::grammar as ast;
use crate::request_grammar::{self as request, Entry, Value as Syntax};
use crate::schema::{Schema, Type, TypeKind};

/// The type `node` has of its own, read from how it is written and the
/// schema's types, never from its methods, so adding an overload never
/// changes how a call reads:
///
/// - a number: `Integer` without a point or an exponent, else `Float`;
/// - a string: `Text` (a `Bytes`, `Instant` or named type is annotated);
/// - `true` or `false`: `Boolean`;
/// - whole numbers each with a unit: `Duration`;
/// - a list: `LIST` of its items' type, when it has items and they all have
///   the same one;
/// - braces of string keys: `MAP` of its values' type, the same way;
/// - braces of fields: the one record of the schema, not generic, that has
///   each field written and is missing none it requires;
/// - a variant: the one choice of the schema, not generic, with a variant
///   of its shape.
///
/// Anything else has none: `[]`, `{}`, a value of a generic type, and a
/// record or variant that several types fit.
pub(crate) fn own_type(schema: &Schema, node: &Node) -> Option<Type> {
    own_syntax_type(schema, &request::decode_value(node).ok()?)
}

pub(crate) fn own_syntax_type(schema: &Schema, value: &Syntax) -> Option<Type> {
    match value {
        Syntax::Integer(_) => Some(Type::Integer),
        Syntax::Float(_) => Some(Type::Float),
        Syntax::Text(_) => Some(Type::Text),
        Syntax::List(items) => same_type(schema, &items.value.iter().collect::<Vec<_>>())
            .map(|item| Type::List(Box::new(item))),
        Syntax::Braces(entries) if entries.value.is_empty() => None,
        Syntax::Braces(entries) => {
            let map_values = entries
                .value
                .iter()
                .map(|entry| match entry {
                    Entry::Map { value, .. } => Some(value),
                    _ => None,
                })
                .collect::<Option<Vec<_>>>();
            if let Some(values) = map_values {
                return same_type(schema, &values).map(|value| Type::Map(Box::new(value)));
            }
            let entries = entries
                .value
                .iter()
                .map(|entry| match entry {
                    Entry::Value(value) => Some(value),
                    _ => None,
                })
                .collect::<Option<Vec<_>>>()?;
            only(schema.types.iter().enumerate().filter(|(_, definition)| {
                let TypeKind::Record(fields) = &definition.kind else {
                    return false;
                };
                if !definition.parameters.is_empty() {
                    return false;
                }
                let Some(filled) = entries
                    .iter()
                    .map(|entry| crate::request::entry_field(schema, fields, entry))
                    .collect::<Option<Vec<_>>>()
                else {
                    return false;
                };
                fields
                    .iter()
                    .enumerate()
                    .all(|(index, field)| field.default.is_some() || filled.contains(&index))
            }))
        }
        Syntax::Duration(duration) => {
            let pairs = duration
                .name
                .words()
                .zip(&duration.arguments)
                .map(|(unit, count)| (count.value.as_str(), unit))
                .collect::<Vec<_>>();
            if crate::time::parse_duration(&pairs).is_ok() {
                return Some(Type::Duration);
            }
            choice_of(schema, &duration.name.text)
        }
        Syntax::Call(call) => {
            let text = &call.name.text;
            choice_of(schema, text)
        }
    }
}

fn choice_of(schema: &Schema, text: &str) -> Option<Type> {
    only(schema.types.iter().enumerate().filter(|(_, definition)| {
        definition.parameters.is_empty()
            && matches!(&definition.kind, TypeKind::Choice(variants)
                if variants.iter().any(|variant| variant.shape.text == text))
    }))
}

fn same_type(schema: &Schema, values: &[&Syntax]) -> Option<Type> {
    let (first, rest) = values.split_first()?;
    let first = own_syntax_type(schema, first)?;
    rest.iter()
        .all(|value| own_syntax_type(schema, value).as_ref() == Some(&first))
        .then_some(first)
}

/// Resolve the request grammar's copy of the schema's type rule with the
/// same semantic resolver. No type syntax is decoded a second time.
pub(crate) fn request_type(schema: &Schema, written: &request::Type) -> Option<Type> {
    fn schema_type(written: &request::Type) -> ast::Type {
        match written {
            request::Type::Refined {
                base,
                predicate,
                span,
            } => ast::Type::Refined {
                base: Box::new(schema_type(base)),
                predicate: ast::Spanned {
                    value: predicate.value.clone(),
                    span: predicate.span,
                },
                span: *span,
            },
            request::Type::List { item, span } => ast::Type::List {
                item: Box::new(schema_type(item)),
                span: *span,
            },
            request::Type::Map { value, span } => ast::Type::Map {
                value: Box::new(schema_type(value)),
                span: *span,
            },
            request::Type::Named(reference) => ast::Type::Named(ast::Reference {
                name: ast::CallName {
                    text: reference.name.text.clone(),
                    parts: reference.name.parts.clone(),
                    span: reference.name.span,
                },
                arguments: reference.arguments.iter().map(schema_type).collect(),
                span: reference.span,
            }),
        }
    }
    resolve(schema, &schema_type(written))
}

/// The type at the one index `found` yields, if it yields exactly one.
fn only<'d, T: 'd>(mut found: impl Iterator<Item = (usize, &'d T)>) -> Option<Type> {
    let (index, _) = found.next()?;
    found.next().is_none().then_some(Type::Named(index))
}

fn resolve(schema: &Schema, written: &ast::Type) -> Option<Type> {
    Some(match written {
        ast::Type::Refined {
            base, predicate, ..
        } => {
            let base = resolve(schema, base)?;
            let predicate = crate::refinement::Predicate::from_nodes(&predicate.value).ok()?;
            predicate.check_type(schema, &base).ok()?;
            Type::Refined(Box::new(base), predicate)
        }
        ast::Type::List { item, .. } => Type::List(Box::new(resolve(schema, item)?)),
        ast::Type::Map { value, .. } => Type::Map(Box::new(resolve(schema, value)?)),
        ast::Type::Named(reference) => {
            if let Some(built_in) = crate::prelude::reference(reference) {
                return Some(built_in);
            }
            if reference
                .name
                .parts
                .iter()
                .any(|part| matches!(part, NamePart::Word { text, .. } if text == "/"))
            {
                return None;
            }
            let index = schema
                .types
                .iter()
                .position(|definition| definition.name == reference.name.text)?;
            let arguments = reference
                .arguments
                .iter()
                .map(|argument| resolve(schema, argument))
                .collect::<Option<Vec<_>>>()?;
            if arguments.is_empty() {
                Type::Named(index)
            } else {
                Type::Generic(index, arguments)
            }
        }
    })
}

/// Whether no annotation could tell a value of `a` from one of `b`: the
/// same type, where a type parameter stands for every type.
pub(crate) fn overlaps(a: &Type, b: &Type) -> bool {
    match (a, b) {
        (Type::Refined(a, _), Type::Refined(b, _)) => overlaps(a, b),
        (Type::Refined(a, _), b) | (b, Type::Refined(a, _)) => overlaps(a, b),
        (Type::Parameter(_), _) | (_, Type::Parameter(_)) => true,
        (Type::List(a), Type::List(b)) | (Type::Map(a), Type::Map(b)) => overlaps(a, b),
        (Type::Generic(a, a_arguments), Type::Generic(b, b_arguments)) => {
            a == b
                && a_arguments.len() == b_arguments.len()
                && a_arguments
                    .iter()
                    .zip(b_arguments)
                    .all(|(a, b)| overlaps(a, b))
        }
        (a, b) => a == b,
    }
}
