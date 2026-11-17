//! Compile expressions from the anyx tree, synthesizing argument types before
//! choosing an overload. No tool is invoked during compilation.

use std::collections::BTreeSet;

use anyx::{Element, Expression, Item, Node, NodeKind, Span, TokenKind};
use anyx_protocol::{
    Problem, Schema, Value,
    schema::{Type, TypeKind},
};

use crate::{Interpreter, at};

pub(crate) struct Call {
    pub scope: String,
    pub method: usize,
    pub arguments: Vec<Argument>,
}

pub(crate) struct Argument {
    pub r#type: Type,
    pub value: ExpressionValue,
}

pub(crate) enum ExpressionValue {
    Literal(Value),
    Call(Box<Call>),
    List(Vec<Argument>),
    Record(Vec<(String, Argument)>),
    Map(Vec<(String, Argument)>),
    Variant(String, Vec<Argument>),
}

pub(crate) struct Compiler<'a> {
    pub interpreter: &'a Interpreter,
    pub source: &'a str,
}

impl Compiler<'_> {
    pub fn element(&self, element: &Element) -> Result<Call, Problem> {
        let end = element
            .comma
            .as_ref()
            .map_or(self.source.len(), |comma| comma.span.start);
        self.call(Span {
            start: expression_start(&element.expression),
            end,
        })
    }

    fn call(&self, span: Span) -> Result<Call, Problem> {
        let fragment = &self.source[span.start..span.end];
        let parsed = anyx::parse(fragment);
        let Some(element) = parsed.syntax.sequence.elements.first() else {
            return Err(self.error(span, "expected a scoped method call"));
        };
        let run = first_run(&element.expression);
        let [Item::Atom(scope), Item::Atom(slash), ..] = run.items.as_slice() else {
            return Err(self.error(
                span,
                "expected a scoped call, such as `environment / list variables`",
            ));
        };
        if scope.kind != TokenKind::Word || slash.kind != TokenKind::Word || slash.text != "/" {
            return Err(self.error(
                span,
                "expected a scoped call, such as `environment / list variables`",
            ));
        }
        let tool = self
            .interpreter
            .tools
            .get(&scope.text)
            .ok_or_else(|| self.error(span, format!("unknown tool scope `{}`", scope.text)))?;
        let schema = tool.schema();
        let body_start = span.start + slash.span.end;
        let body = &self.source[body_start..span.end];
        let parsed = anyx::parse(body);
        if let Some(error) = parsed.errors.first() {
            return Err(at(
                self.source,
                body_start + error.span.start,
                error.message.clone(),
            ));
        }
        let mut nodes = parsed.syntax.nodes();
        let [node] = nodes.as_mut_slice() else {
            return Err(self.error(span, "expected one method call after `/`"));
        };
        relocate(node, body_start);
        let NodeKind::Call(call) = &node.kind else {
            return Err(self.error(node.span, "expected a method call after `/`"));
        };
        let name = call.name_text();
        let methods: Vec<_> = (0..=schema.introspection())
            .filter(|index| schema.method_at(*index).shape.text == name)
            .collect();
        if methods.is_empty() {
            return Err(self.error(
                node.span,
                format!("unknown method `{name}` in `{}`", scope.text),
            ));
        }
        let arguments = call
            .arguments
            .iter()
            .map(|argument| self.argument(schema, argument, None))
            .collect::<Result<Vec<_>, _>>()?;
        let fitting: Vec<_> = methods
            .iter()
            .copied()
            .filter(|index| {
                let parameters = &schema.method_at(*index).shape.parameters;
                parameters.len() == arguments.len()
                    && parameters
                        .iter()
                        .zip(&arguments)
                        .all(|(parameter, argument)| parameter.r#type == argument.r#type)
            })
            .collect();
        let [method] = fitting.as_slice() else {
            let mismatch = methods
                .iter()
                .find_map(|index| {
                    schema
                        .method_at(*index)
                        .shape
                        .parameters
                        .iter()
                        .zip(&arguments)
                        .enumerate()
                        .find(|(_, (parameter, argument))| parameter.r#type != argument.r#type)
                        .map(|(index, _)| call.arguments[index].span)
                })
                .unwrap_or(node.span);
            let found = arguments
                .iter()
                .map(|argument| schema.type_name(&argument.r#type))
                .collect::<Vec<_>>()
                .join(", ");
            let expected = methods
                .iter()
                .map(|index| schema.typed_shape(&schema.method_at(*index).shape))
                .collect::<Vec<_>>()
                .join(", ");
            return Err(self.error(
                mismatch,
                format!("`{name}` has argument types [{found}]; expected one of: {expected}"),
            ));
        };
        Ok(Call {
            scope: scope.text.clone(),
            method: *method,
            arguments,
        })
    }

    fn argument(
        &self,
        schema: &Schema,
        node: &Node,
        expected: Option<&Type>,
    ) -> Result<Argument, Problem> {
        if let NodeKind::Call(call) = &node.kind
            && call.name_text() == "_ AS _"
            && call.arguments.len() == 2
        {
            let written = &call.arguments[1];
            let r#type = self.local(written.span, |text| {
                anyx_protocol::decode_type(schema, text)
            })?;
            let argument = self.argument(schema, &call.arguments[0], Some(&r#type))?;
            return self.check(schema, node.span, argument, expected);
        }
        if self.is_scoped(node) {
            let call = self.call(node.span)?;
            let origin = self.interpreter.tools[&call.scope].schema();
            let returns = origin
                .method_at(call.method)
                .returns
                .as_ref()
                .ok_or_else(|| {
                    self.error(
                        node.span,
                        "a method that returns nothing cannot be used as an argument",
                    )
                })?;
            let r#type = transfer(origin, schema, &returns.r#type, &mut BTreeSet::new())
                .ok_or_else(|| self.error(node.span, format!("the result type `{}` is not available with the same definition in this protocol", origin.type_name(&returns.r#type))))?;
            return self.check(
                schema,
                node.span,
                Argument {
                    r#type,
                    value: ExpressionValue::Call(Box::new(call)),
                },
                expected,
            );
        }
        let literal = match expected {
            Some(expected) => self
                .local(node.span, |text| {
                    anyx_protocol::decode_value(schema, expected, text)
                })
                .map(|value| (expected.clone(), value)),
            None => self.local(node.span, |text| {
                anyx_protocol::decode_argument(schema, text)
            }),
        };
        if let Ok((r#type, value)) = literal {
            return Ok(Argument {
                r#type,
                value: ExpressionValue::Literal(value),
            });
        }
        let literal_error = literal.unwrap_err();
        match &node.kind {
            NodeKind::Brackets(items) => {
                let item_type = match expected.map(|expected| schema.underlying(expected)) {
                    Some(Type::List(item)) => Some(item.as_ref()),
                    Some(_) => return Err(literal_error),
                    None => None,
                };
                let arguments = items
                    .iter()
                    .map(|node| self.argument(schema, node, item_type))
                    .collect::<Result<Vec<_>, _>>()?;
                let Some(first) = arguments.first() else {
                    return Err(literal_error);
                };
                if arguments
                    .iter()
                    .any(|argument| argument.r#type != first.r#type)
                {
                    return Err(self.error(node.span, "list items must have the same type"));
                }
                let r#type = expected
                    .cloned()
                    .unwrap_or_else(|| Type::List(Box::new(first.r#type.clone())));
                Ok(Argument {
                    r#type,
                    value: ExpressionValue::List(arguments),
                })
            }
            NodeKind::Braces(entries) => self.braces(schema, node.span, entries, expected),
            NodeKind::Call(call) => {
                let name = call.name_text();
                let candidates: Vec<_> = schema.types.iter().enumerate().filter_map(|(index, definition)| {
                    if definition.parameters.is_empty() && matches!(&definition.kind, TypeKind::Choice(variants) if variants.iter().any(|variant| variant.shape.text == name)) {
                        Some(Type::Named(index))
                    } else { None }
                }).collect();
                let r#type = match (expected, candidates.as_slice()) {
                    (Some(expected), _) => expected.clone(),
                    (None, [only]) => only.clone(),
                    _ => return Err(literal_error),
                };
                let Some((_, kind)) = schema.instance(&r#type) else {
                    return Err(literal_error);
                };
                let TypeKind::Choice(variants) = kind.as_ref() else {
                    return Err(literal_error);
                };
                let Some(variant) = variants.iter().find(|variant| variant.shape.text == name)
                else {
                    return Err(literal_error);
                };
                if call.arguments.len() != variant.shape.parameters.len() {
                    return Err(literal_error);
                }
                let arguments = call
                    .arguments
                    .iter()
                    .zip(&variant.shape.parameters)
                    .map(|(node, parameter)| self.argument(schema, node, Some(&parameter.r#type)))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(Argument {
                    r#type,
                    value: ExpressionValue::Variant(name, arguments),
                })
            }
            _ => Err(literal_error),
        }
    }

    fn braces(
        &self,
        schema: &Schema,
        span: Span,
        entries: &[Node],
        expected: Option<&Type>,
    ) -> Result<Argument, Problem> {
        if entries.is_empty() && expected.is_none() {
            return Err(self.error(
                span,
                "empty braces have no type of their own; annotate them with `AS: Type`",
            ));
        }
        let mut fields = Vec::new();
        let mut is_map = false;
        for entry in entries {
            let NodeKind::Call(call) = &entry.kind else {
                return Err(self.error(entry.span, "expected a record field or map entry"));
            };
            let (name, value) = if call.name_text() == "_ _" && call.arguments.len() == 2 {
                let NodeKind::String(key) = &call.arguments[0].kind else {
                    return Err(self.error(entry.span, "expected a text map key"));
                };
                is_map = true;
                (key.value.clone(), &call.arguments[1])
            } else if call.arguments.len() == 1 && call.name.last() == Some(&anyx::NamePart::Slot) {
                (
                    call.name_text().trim_end_matches(" _").to_string(),
                    &call.arguments[0],
                )
            } else {
                return Err(self.error(entry.span, "expected one value per record field"));
            };
            if fields.iter().any(|(previous, _)| previous == &name) {
                return Err(self.error(entry.span, format!("duplicate field or key `{name}`")));
            }
            fields.push((name, value));
        }
        if is_map {
            if entries.iter().any(
                |entry| !matches!(&entry.kind, NodeKind::Call(call) if call.name_text() == "_ _"),
            ) {
                return Err(self.error(span, "cannot mix record fields and map keys"));
            }
            let item = match expected.map(|expected| schema.underlying(expected)) {
                Some(Type::Map(item)) => Some(item.as_ref()),
                Some(_) => return Err(self.error(span, "expected a record, found a map")),
                None => None,
            };
            let values = fields
                .into_iter()
                .map(|(key, node)| Ok((key, self.argument(schema, node, item)?)))
                .collect::<Result<Vec<_>, Problem>>()?;
            let Some((_, first)) = values.first() else {
                return Err(self.error(span, "annotate an empty map"));
            };
            if values.iter().any(|(_, value)| value.r#type != first.r#type) {
                return Err(self.error(span, "map values must have the same type"));
            }
            let r#type = expected
                .cloned()
                .unwrap_or_else(|| Type::Map(Box::new(first.r#type.clone())));
            return Ok(Argument {
                r#type,
                value: ExpressionValue::Map(values),
            });
        }
        let candidates: Vec<_> = schema
            .types
            .iter()
            .enumerate()
            .filter_map(|(index, definition)| {
                let TypeKind::Record(record) = &definition.kind else {
                    return None;
                };
                (definition.parameters.is_empty()
                    && fields
                        .iter()
                        .all(|(name, _)| record.iter().any(|field| field.name == *name))
                    && record.iter().all(|field| {
                        field.optional || fields.iter().any(|(name, _)| field.name == *name)
                    }))
                .then_some(Type::Named(index))
            })
            .collect();
        let r#type = match (expected, candidates.as_slice()) {
            (Some(expected), _) => expected.clone(),
            (None, [only]) => only.clone(),
            _ => {
                return Err(self.error(
                    span,
                    "record has no unambiguous type; annotate it with `AS: Type`",
                ));
            }
        };
        let Some((_, kind)) = schema.instance(&r#type) else {
            return Err(self.error(span, "expected a record type"));
        };
        let TypeKind::Record(record) = kind.as_ref() else {
            return Err(self.error(span, "expected a record type"));
        };
        let mut values = fields
            .into_iter()
            .map(|(name, node)| {
                let field = record
                    .iter()
                    .find(|field| field.name == name)
                    .ok_or_else(|| self.error(node.span, format!("unknown field `{name}`")))?;
                Ok((name, self.argument(schema, node, Some(&field.r#type))?))
            })
            .collect::<Result<Vec<_>, Problem>>()?;
        for field in record {
            if !values.iter().any(|(name, _)| *name == field.name) {
                if let Some(value) = &field.default {
                    values.push((
                        field.name.clone(),
                        Argument {
                            r#type: field.r#type.clone(),
                            value: ExpressionValue::Literal(value.clone()),
                        },
                    ));
                } else if !field.optional {
                    return Err(
                        self.error(span, format!("missing required field `{}`", field.name))
                    );
                }
            }
        }
        Ok(Argument {
            r#type,
            value: ExpressionValue::Record(values),
        })
    }

    fn check(
        &self,
        schema: &Schema,
        span: Span,
        argument: Argument,
        expected: Option<&Type>,
    ) -> Result<Argument, Problem> {
        if let Some(expected) = expected.filter(|expected| **expected != argument.r#type) {
            return Err(self.error(
                span,
                format!(
                    "expression returns `{}`; expected `{}`",
                    schema.type_name(&argument.r#type),
                    schema.type_name(expected)
                ),
            ));
        }
        Ok(argument)
    }

    fn is_scoped(&self, node: &Node) -> bool {
        let parsed = anyx::parse(&self.source[node.span.start..node.span.end]);
        parsed.syntax.sequence.elements.first().is_some_and(|element| {
            matches!(first_run(&element.expression).items.as_slice(),
                [Item::Atom(scope), Item::Atom(slash), ..]
                if scope.kind == TokenKind::Word && slash.kind == TokenKind::Word && slash.text == "/")
        })
    }

    fn local<T>(
        &self,
        span: Span,
        decode: impl FnOnce(&str) -> Result<T, Problem>,
    ) -> Result<T, Problem> {
        decode(&self.source[span.start..span.end]).map_err(|problem| {
            let base = anyx::line_column(self.source, span.start);
            Problem {
                line: base.line + problem.line - 1,
                column: if problem.line == 1 {
                    base.column + problem.column - 1
                } else {
                    problem.column
                },
                message: problem.message,
            }
        })
    }

    fn error(&self, span: Span, message: impl Into<String>) -> Problem {
        at(self.source, span.start, message.into())
    }
}

fn first_run(expression: &Expression) -> &anyx::Run {
    match expression {
        Expression::Run(run) => run,
        Expression::Dot(binary) | Expression::Colon(binary) => first_run(&binary.left),
    }
}

fn expression_start(expression: &Expression) -> usize {
    first_run(expression).position
}

fn relocate(node: &mut Node, offset: usize) {
    node.span.start += offset;
    node.span.end += offset;
    match &mut node.kind {
        NodeKind::Call(call) => {
            for part in &mut call.name {
                if let anyx::NamePart::Word { span, .. } = part {
                    span.start += offset;
                    span.end += offset;
                }
            }
            for argument in &mut call.arguments {
                relocate(argument, offset);
            }
        }
        NodeKind::Brackets(items) | NodeKind::Braces(items) | NodeKind::Parens(items) => {
            for item in items {
                relocate(item, offset);
            }
        }
        _ => {}
    }
}

/// Translate protocol-local type indices by name, checking the complete
/// reachable definitions so unrelated types with the same name cannot mix.
fn transfer(
    origin: &Schema,
    target: &Schema,
    r#type: &Type,
    visited: &mut BTreeSet<(usize, usize)>,
) -> Option<Type> {
    if std::ptr::eq(origin, target) {
        return Some(r#type.clone());
    }
    Some(match r#type {
        Type::List(item) => Type::List(Box::new(transfer(origin, target, item, visited)?)),
        Type::Map(item) => Type::Map(Box::new(transfer(origin, target, item, visited)?)),
        Type::Named(index) | Type::Generic(index, _) => {
            let definition = &origin.types[*index];
            let other = target.types.iter().position(|other| {
                other.name == definition.name && other.parameters == definition.parameters
            })?;
            if visited.insert((*index, other)) {
                let fits = match (&definition.kind, &target.types[other].kind) {
                    (TypeKind::Distinct(left), TypeKind::Distinct(right)) => {
                        transfer(origin, target, left, visited)? == *right
                    }
                    (TypeKind::Record(left), TypeKind::Record(right)) => {
                        left.len() == right.len()
                            && left.iter().zip(right).all(|(left, right)| {
                                left.name == right.name
                                    && left.optional == right.optional
                                    && left.default == right.default
                                    && transfer(origin, target, &left.r#type, visited).as_ref()
                                        == Some(&right.r#type)
                            })
                    }
                    (TypeKind::Choice(left), TypeKind::Choice(right)) => {
                        left.len() == right.len()
                            && left.iter().zip(right).all(|(left, right)| {
                                left.shape.text == right.shape.text
                                    && left.shape.parameters.len() == right.shape.parameters.len()
                                    && left
                                        .shape
                                        .parameters
                                        .iter()
                                        .zip(&right.shape.parameters)
                                        .all(|(left, right)| {
                                            transfer(origin, target, &left.r#type, visited).as_ref()
                                                == Some(&right.r#type)
                                        })
                            })
                    }
                    _ => false,
                };
                if !fits {
                    return None;
                }
            }
            match r#type {
                Type::Generic(_, arguments) => Type::Generic(
                    other,
                    arguments
                        .iter()
                        .map(|argument| transfer(origin, target, argument, visited))
                        .collect::<Option<Vec<_>>>()?,
                ),
                _ => Type::Named(other),
            }
        }
        _ => r#type.clone(),
    })
}
