//! Schema checking of the fixed request syntax. No raw anyx nodes are read here.
use std::collections::BTreeMap;

use crate::codec::{DURATION_EXAMPLE, INSTANT_EXAMPLE, Mismatch};
use crate::request_grammar::{Entry, Value as Syntax};
use crate::schema::{Field, FieldForm, Schema, Shape, ShapePart, Type, TypeKind, Written};
use crate::value::Value;

fn describe(value: &Syntax) -> String {
    match value {
        Syntax::Integer(value) | Syntax::Float(value) => format!("the number `{}`", value.value),
        Syntax::Text(_) => "a string".into(),
        Syntax::List(_) => "a list `[…]`".into(),
        Syntax::Braces(_) => "braces `{…}`".into(),
        Syntax::Duration(duration) => format!("the call `{}`", duration.name.text),
        Syntax::Call(call) => format!("the call `{}`", call.name.text),
    }
}

pub(crate) fn decode(schema: &Schema, expected: &Type, value: &Syntax) -> Result<Value, Mismatch> {
    let decoded = decode_raw(schema, expected, value)?;
    crate::refinement::validate(schema, expected, &decoded).map_err(|message| Mismatch {
        span: value.span(),
        message,
    })?;
    Ok(decoded)
}

fn decode_raw(schema: &Schema, expected: &Type, value: &Syntax) -> Result<Value, Mismatch> {
    if let Syntax::Duration(duration) = value {
        return decode(schema, expected, &duration_call(duration));
    }
    let expected = schema.underlying(expected);
    let mismatch = |expected: String| Mismatch {
        span: value.span(),
        message: format!("found {}; expected {expected}", describe(value)),
    };
    match (expected, value) {
        (Type::Tuple(_), _) => Err(mismatch("a field's slots, written in its shape".into())),
        (Type::Text, Syntax::Text(text)) => Ok(Value::Text(text.value.value.clone())),
        (Type::Text, _) => Err(mismatch("a string, as in `\"text\"`".into())),
        (Type::Bytes, Syntax::Text(text)) => crate::bytes::decode(&text.value.value)
            .map(Value::Bytes)
            .ok_or_else(|| mismatch("base64 in the string, as in `\"TVRoZA==\"`".into())),
        (Type::Bytes, _) => Err(mismatch("base64 in a string, as in `\"TVRoZA==\"`".into())),
        (Type::Integer, Syntax::Float(n)) => Err(Mismatch {
            span: n.span,
            message: format!(
                "found the Float `{}`; expected an Integer, a whole number written without a point, such as `3`",
                n.value
            ),
        }),
        (Type::Integer, Syntax::Integer(n)) => n
            .value
            .parse()
            .map(Value::Integer)
            .map_err(|_| mismatch("a whole number that fits in 64 bits".into())),
        (Type::Integer, _) => Err(mismatch("a whole number, as in `3`".into())),
        (Type::Float, Syntax::Integer(n)) => Err(Mismatch {
            span: n.span,
            message: format!(
                "found the Integer `{0}`; expected a Float, such as `{0}.0`",
                n.value
            ),
        }),
        (Type::Float, Syntax::Float(n)) => n
            .value
            .parse()
            .ok()
            .filter(|n: &f64| n.is_finite())
            .map(Value::Float)
            .ok_or_else(|| mismatch("a finite Float, as in `1.5`".into())),
        (Type::Float, _) => Err(mismatch("a Float, as in `1.5`".into())),
        (Type::Instant, Syntax::Text(text)) => crate::time::parse_instant(&text.value.value)
            .map(Value::Instant)
            .ok_or_else(|| {
                mismatch(format!(
                    "an instant in UTC in the string, as in `\"{INSTANT_EXAMPLE}\"`"
                ))
            }),
        (Type::Instant, _) => Err(mismatch(format!(
            "an instant in UTC in a string, as in `\"{INSTANT_EXAMPLE}\"`"
        ))),
        (Type::Duration, Syntax::Call(call)) => {
            let pairs = duration_pairs(call).ok_or_else(|| {
                mismatch(format!(
                    "a duration, whole numbers each with a unit, as in `{DURATION_EXAMPLE}`"
                ))
            })?;
            crate::time::parse_duration(&pairs).map(Value::Duration).map_err(|found| Mismatch {
                span: value.span(),
                message: format!("found {found}; expected a duration, whole numbers each with a unit, the largest first, as in `{DURATION_EXAMPLE}`; the units are {}", crate::time::units().iter().map(|unit| format!("`{}`", unit.plural)).collect::<Vec<_>>().join(", ")),
            })
        }
        (Type::Duration, _) => Err(mismatch(format!(
            "a duration, as in `{DURATION_EXAMPLE}`, which goes in parentheses in a slot"
        ))),
        (Type::List(item), Syntax::List(items)) => items
            .value
            .iter()
            .map(|value| decode(schema, item, value))
            .collect::<Result<_, _>>()
            .map(Value::List),
        (Type::List(item), _) => Err(mismatch(format!(
            "a list of {}, as in `[…]`",
            schema.type_name(item)
        ))),
        (Type::Map(item), Syntax::Braces(entries)) => {
            let mut values = BTreeMap::new();
            for entry in &entries.value {
                let Entry::Map { key, value, span } = entry else {
                    let found = match entry {
                        Entry::Value(value) => describe(value),
                        Entry::Map { .. } => unreachable!(),
                    };
                    return Err(Mismatch {
                        span: entry.span(),
                        message: format!(
                            "found {found}; expected a map entry, a string key and its value, as in `\"key\": …`"
                        ),
                    });
                };
                let key = &key.value.value;
                if values.contains_key(key) {
                    return Err(Mismatch {
                        span: *span,
                        message: format!("found the key `\"{key}\"` twice; expected each key once"),
                    });
                }
                let value = decode(schema, item, value).map_err(|m| Mismatch {
                    message: format!("in `\"{key}\"`: {}", m.message),
                    ..m
                })?;
                values.insert(key.clone(), value);
            }
            Ok(Value::Map(values))
        }
        (Type::Map(item), _) => Err(mismatch(format!(
            "a map of {}, as in `{{ \"key\" … }}`",
            schema.type_name(item)
        ))),
        (Type::Named(_) | Type::Generic(..), _) => {
            let (definition, kind) = schema
                .instance(expected)
                .expect("a checked type has a definition");
            match kind.as_ref() {
                TypeKind::Record(fields) => match value {
                    Syntax::Braces(entries) => {
                        fields_value(schema, fields, &entries.value, entries.span)
                            .map(Value::Record)
                    }
                    _ => Err(mismatch(format!(
                        "the record `{}`, as in `{{ {} }}`",
                        definition.shape(),
                        fields
                            .iter()
                            .map(|f| format!("{} …", f.name))
                            .collect::<Vec<_>>()
                            .join(", ")
                    ))),
                },
                TypeKind::Choice(variants) => variant(
                    schema,
                    &variants.iter().map(|v| &v.shape).collect::<Vec<_>>(),
                    value,
                    &format!("a variant of `{}`", definition.shape()),
                ),
                TypeKind::Distinct(_) => unreachable!("underlying reads past named types"),
            }
        }
        (Type::Union(_), _) => match schema.member_written_as(expected, &written(value)) {
            Some(member) => decode(schema, member, value),
            None => Err(mismatch(format!(
                "a value of one of {}",
                expected
                    .members()
                    .iter()
                    .map(|member| format!("`{}`", schema.type_name(member)))
                    .collect::<Vec<_>>()
                    .join(", ")
            ))),
        },
        (Type::Variant(shape), _) => variant(
            schema,
            &[shape.as_ref()],
            value,
            &format!("`{}`", schema.shape_name(shape)),
        ),
        (Type::Refined(..), _) => unreachable!("underlying removes refinements"),
        (Type::Parameter(name), _) => Err(mismatch(format!(
            "a value of a type in place of the type parameter `{name}`"
        ))),
    }
}

pub(crate) fn duration_pairs(
    call: &crate::request_grammar::ValueCall,
) -> Option<Vec<(&str, &str)>> {
    use anyx::NamePart;
    if call.name.parts.len() != call.arguments.len() * 2 {
        return None;
    }
    call.name
        .parts
        .chunks(2)
        .zip(&call.arguments)
        .map(|(parts, value)| match (parts, value) {
            (
                [NamePart::Slot, NamePart::Word { text, .. }],
                Syntax::Integer(n) | Syntax::Float(n),
            ) => Some((n.value.as_str(), text.as_str())),
            _ => None,
        })
        .collect()
}

pub(crate) fn variant(
    schema: &Schema,
    shapes: &[&Shape],
    value: &Syntax,
    what: &str,
) -> Result<Value, Mismatch> {
    if let Syntax::Duration(duration) = value {
        return variant(schema, shapes, &duration_call(duration), what);
    }
    if let Syntax::Call(call) = value
        && let Some(shape) = shapes.iter().find(|shape| shape.text == call.name.text)
    {
        let arguments = shape
            .parameters
            .iter()
            .zip(&call.arguments)
            .map(|(parameter, argument)| {
                decode(schema, &parameter.r#type, argument).map_err(|m| Mismatch {
                    message: format!(
                        "in `{}` of `{}`: {}",
                        parameter.name, call.name.text, m.message
                    ),
                    ..m
                })
            })
            .collect::<Result<_, _>>()?;
        return Ok(Value::Variant {
            shape: shape.text.clone(),
            arguments,
        });
    }
    Err(Mismatch {
        span: value.span(),
        message: format!(
            "found {}; expected {what}: {}",
            describe(value),
            shapes
                .iter()
                .map(|shape| format!("`{}`", shape.text))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    })
}

fn fields_value(
    schema: &Schema,
    fields: &[Field],
    entries: &[Entry],
    span: anyx::Span,
) -> Result<BTreeMap<String, Value>, Mismatch> {
    let mut values = BTreeMap::new();
    for entry in entries {
        let Entry::Value(entry) = entry else {
            return Err(Mismatch {
                span: entry.span(),
                message: "found a map entry; expected a record's field, as in `name …`".into(),
            });
        };
        let (field, value) = field_entry(schema, fields, entry)?;
        let name = &field.name;
        if values.contains_key(name) {
            return Err(Mismatch {
                span: entry.span(),
                message: format!("found the field `{name}` twice; expected each field once"),
            });
        }
        values.insert(name.clone(), value);
    }
    for field in fields {
        if let (Some(default), false) = (&field.default, values.contains_key(&field.name)) {
            values.insert(field.name.clone(), default.clone());
        }
    }
    let missing = fields
        .iter()
        .filter(|field| field.default.is_none() && !values.contains_key(&field.name))
        .map(|field| format!("`{}`", field.name))
        .collect::<Vec<_>>();
    if !missing.is_empty() {
        return Err(Mismatch {
            span,
            message: format!(
                "found no {}; expected every field without a `DEFAULT`",
                missing.join(", ")
            ),
        });
    }
    Ok(values)
}

/// The field an entry of a record fills, by the entry's shape alone:
/// `path "a"` is the shape `path _`, `hidden` a checkbox, and `@newest first`
/// the `@` field whose values are written in the form of `newest first`.
/// The schema checker makes sure at most one fits, so nothing is tried.
pub(crate) fn entry_field(schema: &Schema, fields: &[Field], entry: &Syntax) -> Option<usize> {
    let Syntax::Call(call) = entry else {
        return None;
    };
    match call.name.text.as_str() {
        "@ _" => schema.unnamed_field(fields, &written(&call.arguments[0])),
        shape => schema.shaped_field(fields, shape),
    }
}

/// The field an entry of a record fills, and its value.
fn field_entry<'f>(
    schema: &Schema,
    fields: &'f [Field],
    entry: &Syntax,
) -> Result<(&'f Field, Value), Mismatch> {
    if let Some(index) = entry_field(schema, fields, entry) {
        let field = &fields[index];
        let value = entry_value(schema, field, entry).map_err(|m| Mismatch {
            message: format!("in `{}`: {}", field.name, m.message),
            ..m
        })?;
        return Ok((field, value));
    }
    // `any case` for the shape `_ case`: the value before the words needs
    // its parentheses, or it joins them.
    let hint = match entry {
        Syntax::Call(call) if call.arguments.is_empty() => fields.iter().find_map(|field| {
            let FieldForm::Shape(shape) = &field.form else {
                return None;
            };
            let words = shape.strip_prefix("_ ")?;
            let value = call.name.text.strip_suffix(words)?.strip_suffix(' ')?;
            Some(format!(
                "; a value before a field's words goes in parentheses: `({value}) {words}`"
            ))
        }),
        _ => None,
    };
    Err(Mismatch {
        span: entry.span(),
        message: format!(
            "found {}; expected a field: {}{}",
            describe(entry),
            fields
                .iter()
                .map(|field| match &field.form {
                    FieldForm::Shape(shape) => format!("`{}`", shape.replace('_', "…")),
                    FieldForm::Checkbox => format!("`{}`", field.name),
                    FieldForm::Unnamed => format!("`@…` for `{}`", field.name),
                })
                .collect::<Vec<_>>()
                .join(", "),
            hint.unwrap_or_default()
        ),
    })
}

/// The value of `field` that `entry`, an entry found to fill it, holds:
/// one for each slot of a field with several.
fn entry_value(schema: &Schema, field: &Field, entry: &Syntax) -> Result<Value, Mismatch> {
    let Syntax::Call(call) = entry else {
        unreachable!("a field is found by its entry's call")
    };
    match (&field.form, &field.r#type) {
        (FieldForm::Checkbox, _) => Ok(Value::Variant {
            shape: "true".into(),
            arguments: Vec::new(),
        }),
        (_, Type::Tuple(slots)) => slots
            .iter()
            .zip(&call.arguments)
            .map(|(slot, argument)| decode(schema, slot, argument))
            .collect::<Result<_, _>>()
            .map(Value::Tuple),
        (_, r#type) => decode(schema, r#type, &call.arguments[0]),
    }
}

/// Reads `entry` as an entry of `fields[index]`, as a field's `DEFAULT`
/// is: `age 0` for `age (Integer) DEFAULT age 0`.
pub(crate) fn decode_field_entry(
    schema: &Schema,
    fields: &[Field],
    index: usize,
    entry: &Syntax,
) -> Result<Value, Mismatch> {
    let field = &fields[index];
    match entry_field(schema, fields, entry) {
        Some(found) if found == index => entry_value(schema, field, entry),
        _ => Err(Mismatch {
            span: entry.span(),
            message: format!(
                "found {}; expected an entry of the field, as a value writes it: {}",
                describe(entry),
                match &field.form {
                    FieldForm::Shape(shape) => format!("`{}`", shape.replace('_', "…")),
                    FieldForm::Checkbox => format!("`{}`", field.name),
                    FieldForm::Unnamed => "`@…`".to_string(),
                }
            ),
        }),
    }
}

/// How `value` is written, to find the `@` field it fills.
fn written(value: &Syntax) -> Written {
    match value {
        Syntax::Integer(_) | Syntax::Float(_) => Written::Number,
        Syntax::Text(_) => Written::String,
        Syntax::List(_) => Written::List,
        Syntax::Braces(_) => Written::Braces,
        Syntax::Duration(duration) => Written::Call(ShapePart::of(&duration.name.parts)),
        Syntax::Call(call) => Written::Call(ShapePart::of(&call.name.parts)),
    }
}

// Preserve the neutral call reading: `(5 minutes)` may be a choice's
// `(count) minutes` variant instead of a Duration.
fn duration_call(duration: &crate::request_grammar::Duration) -> Syntax {
    Syntax::Call(crate::request_grammar::ValueCall {
        name: duration.name.clone(),
        arguments: duration
            .arguments
            .iter()
            .cloned()
            .map(Syntax::Integer)
            .collect(),
        span: duration.span,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::request_grammar as syntax;
    use crate::{Call, decode_request, encode_request};

    #[test]
    fn annotation_types_use_the_schema_type_syntax() {
        for source in [
            "Text",
            "LIST Text",
            "MAP (LIST Text)",
            "common/File Id",
            "Page of (LIST Text)",
            "common/Page of (Text)",
            "(Integer) or (Text)",
        ] {
            let nodes = anyx::parse(source).syntax.nodes();
            let schema = crate::grammar::decode_type(&nodes[0]).unwrap();
            let request = syntax::decode_type(&nodes[0]).unwrap();
            assert_eq!(format!("{schema:?}"), format!("{request:?}"), "{source}");
        }
        for source in ["LIST", "MAP", "LIST TEXT", "[Text]", "42"] {
            let nodes = anyx::parse(source).syntax.nodes();
            assert!(crate::grammar::decode_type(&nodes[0]).is_err(), "{source}");
            assert!(syntax::decode_type(&nodes[0]).is_err(), "{source}");
        }
    }

    #[test]
    fn duration_syntax_covers_every_prelude_unit() {
        for unit in crate::time::units() {
            for word in [unit.singular, &unit.plural] {
                let source = format!("1 {word}");
                let nodes = anyx::parse(&source).syntax.nodes();
                assert!(
                    matches!(
                        syntax::decode_value(&nodes[0]).unwrap(),
                        Syntax::Duration(_)
                    ),
                    "{source}"
                );
            }
        }
    }

    #[test]
    fn durations_keep_their_choice_reading_with_imported_types() {
        let schema = Schema::load("protocol.protocol.anyx", |path| -> Result<String, String> {
            match path {
                "protocol.protocol.anyx" => Ok("PROTOCOL a VERSION 1, USE \"./files.protocol.anyx\"".into()),
                "files.protocol.anyx" => Ok("PUBLIC CHOICE Count { (count: Integer) minutes }, METHOD count (count: Count) RETURNS NOTHING".into()),
                _ => Err(format!("missing {path}")),
            }
        }).unwrap();
        let calls = vec![Call {
            method: 0,
            arguments: vec![Value::Variant {
                shape: "_ minutes".into(),
                arguments: vec![Value::Integer(5)],
            }],
        }];
        let source = "count (5 minutes AS: Count)";
        assert_eq!(decode_request(&schema, source).unwrap(), calls);
        assert_eq!(
            decode_request(&schema, &encode_request(&schema, &calls).unwrap()).unwrap(),
            calls
        );
    }

    #[test]
    fn booleans_keep_their_choice_reading_when_annotated() {
        let schema = Schema::parse("PROTOCOL a VERSION 1, CHOICE Truth { true, false }, METHOD set (value: Truth) RETURNS NOTHING").unwrap();
        let calls = decode_request(&schema, "set (true AS: Truth)").unwrap();
        assert_eq!(
            calls[0].arguments,
            vec![Value::Variant {
                shape: "true".into(),
                arguments: vec![]
            }]
        );
        assert_eq!(
            decode_request(&schema, &encode_request(&schema, &calls).unwrap()).unwrap(),
            calls
        );
    }

    #[test]
    fn annotations_are_only_whole_arguments() {
        for source in [
            "set ([1 AS: Integer])",
            "set { name: (\"a\" AS: Text) }",
            "set (custom (1 AS: Integer))",
            "set ((1 AS: Integer) AS: Integer)",
        ] {
            let nodes = anyx::parse(source).syntax.nodes();
            assert!(syntax::decode_document(&nodes).is_err(), "{source}");
        }
        let nodes = anyx::parse("set ([1] AS: LIST Integer)").syntax.nodes();
        assert!(syntax::decode_document(&nodes).is_ok());
    }
}
