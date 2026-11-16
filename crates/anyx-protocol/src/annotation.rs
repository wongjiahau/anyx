//! The types of a request's arguments: the type a value has of its own,
//! from how it is written, and the type an annotation, `value AS: type`,
//! claims for it. A call resolves to the overload of its shape whose
//! parameters have exactly those types.

use anyx::{NamePart, Node, NodeKind};

use crate::codec::{duration_pairs, is_float_text};
use crate::grammar as ast;
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
    match &node.kind {
        NodeKind::Number(text) => Some(match is_float_text(text) {
            true => Type::Float,
            false => Type::Integer,
        }),
        NodeKind::String(_) => Some(Type::Text),
        NodeKind::Brackets(items) => same_type(schema, &items.iter().collect::<Vec<_>>())
            .map(|item| Type::List(Box::new(item))),
        NodeKind::Braces(entries) if entries.is_empty() => None,
        NodeKind::Braces(entries) => match map_values(entries) {
            Some(values) => same_type(schema, &values).map(|value| Type::Map(Box::new(value))),
            None => record_of(schema, entries),
        },
        NodeKind::Call(call) => {
            let text = call.name_text();
            if call.arguments.is_empty() && (text == "true" || text == "false") {
                return Some(Type::Boolean);
            }
            if duration_pairs(call).is_some_and(|pairs| crate::time::parse_duration(&pairs).is_ok())
            {
                return Some(Type::Duration);
            }
            only(schema.types.iter().enumerate().filter(|(_, definition)| {
                definition.parameters.is_empty()
                    && matches!(&definition.kind, TypeKind::Choice(variants)
                        if variants.iter().any(|variant| variant.shape.text == text))
            }))
        }
        _ => None,
    }
}

/// The one type all of `nodes` have of their own, if there are any.
fn same_type(schema: &Schema, nodes: &[&Node]) -> Option<Type> {
    let (first, rest) = nodes.split_first()?;
    let first = own_type(schema, first)?;
    rest.iter()
        .all(|node| own_type(schema, node).as_ref() == Some(&first))
        .then_some(first)
}

/// The values of braces of entries, each a string key and its value, or
/// `None` when an entry is not one.
fn map_values(entries: &[Node]) -> Option<Vec<&Node>> {
    entries
        .iter()
        .map(|entry| match &entry.kind {
            NodeKind::Call(call) => match (&call.name[..], &call.arguments[..]) {
                ([NamePart::Slot, NamePart::Slot], [key, value])
                    if matches!(key.kind, NodeKind::String(_)) =>
                {
                    Some(value)
                }
                _ => None,
            },
            _ => None,
        })
        .collect()
}

/// The one record, not generic, that braces of the fields `entries` fit:
/// it has each field written, and none it requires is missing.
fn record_of(schema: &Schema, entries: &[Node]) -> Option<Type> {
    let written: Vec<String> = entries
        .iter()
        .map(|entry| match &entry.kind {
            NodeKind::Call(call) if call.arguments.len() == 1 => {
                call.name_text().strip_suffix(" _").map(str::to_string)
            }
            _ => None,
        })
        .collect::<Option<_>>()?;
    only(schema.types.iter().enumerate().filter(|(_, definition)| {
        let TypeKind::Record(fields) = &definition.kind else {
            return false;
        };
        definition.parameters.is_empty()
            && written
                .iter()
                .all(|name| fields.iter().any(|field| &field.name == name))
            && fields
                .iter()
                .all(|field| field.optional || written.contains(&field.name))
    }))
}

/// The type at the one index `found` yields, if it yields exactly one.
fn only<'d, T: 'd>(mut found: impl Iterator<Item = (usize, &'d T)>) -> Option<Type> {
    let (index, _) = found.next()?;
    found.next().is_none().then_some(Type::Named(index))
}

/// `node` as an annotation, `value AS: type`: the value and the type as
/// written.
pub(crate) fn annotation(node: &Node) -> Option<(&Node, &Node)> {
    let NodeKind::Call(call) = &node.kind else {
        return None;
    };
    match (&call.name[..], &call.arguments[..]) {
        ([NamePart::Slot, NamePart::Word { text, .. }, NamePart::Slot], [value, r#type])
            if text == "AS" =>
        {
            Some((value, r#type))
        }
        _ => None,
    }
}

/// A type written after `AS`, resolved in the protocol's flat namespace.
pub(crate) fn annotated_type(schema: &Schema, node: &Node) -> Option<Type> {
    resolve(schema, &ast::decode_type(node).ok()?)
}

fn resolve(schema: &Schema, written: &ast::Type) -> Option<Type> {
    Some(match written {
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
