//! Compile expressions from the anyx tree, synthesizing argument types before
//! choosing an overload. No tool is invoked during compilation.

use std::collections::BTreeSet;

use anyx::{Expression, Item, Node, NodeKind, Span, TokenKind};
use anyx_protocol::{
    Problem, Schema, Value,
    schema::{Field, FieldForm, Form, Shape, Type, TypeKind, Written},
};

use crate::{Interpreter, at};

pub(crate) struct Call {
    pub scope: String,
    pub method: usize,
    pub arguments: Vec<Argument>,
    /// Where the call is written, for a precondition that fails.
    pub span: Span,
}

/// The arguments of a call of the method at `method`, written in `form`:
/// the `written` ones, and each left-out part's default, as the codec fills
/// them in for a request.
fn filled(schema: &Schema, method: usize, form: &Form, written: Vec<Argument>) -> Vec<Argument> {
    let shape = &schema.method_at(method).shape;
    shape
        .fill_with(form, written, |index, value| Argument {
            r#type: shape.parameters[index].r#type.clone(),
            value: ExpressionValue::Literal(value.clone()),
        })
        .expect("a checked schema has read every default")
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
    /// The slots of a record's field with several of them.
    Tuple(Vec<Argument>),
}

impl Argument {
    /// Its value, when it is written out with no call in it.
    pub fn literal(&self) -> Option<Value> {
        Some(match &self.value {
            ExpressionValue::Literal(value) => value.clone(),
            ExpressionValue::Call(_) => return None,
            ExpressionValue::List(items) => {
                Value::List(items.iter().map(Self::literal).collect::<Option<_>>()?)
            }
            ExpressionValue::Record(fields) => Value::Record(
                fields
                    .iter()
                    .map(|(name, value)| Some((name.clone(), value.literal()?)))
                    .collect::<Option<_>>()?,
            ),
            ExpressionValue::Map(fields) => Value::Map(
                fields
                    .iter()
                    .map(|(name, value)| Some((name.clone(), value.literal()?)))
                    .collect::<Option<_>>()?,
            ),
            ExpressionValue::Variant(shape, arguments) => Value::Variant {
                shape: shape.clone(),
                arguments: arguments.iter().map(Self::literal).collect::<Option<_>>()?,
            },
            ExpressionValue::Tuple(slots) => {
                Value::Tuple(slots.iter().map(Self::literal).collect::<Option<_>>()?)
            }
        })
    }

    /// Runs `check` on each call in it, outermost first.
    fn each_call(
        &self,
        check: &mut impl FnMut(&Call) -> Result<(), Problem>,
    ) -> Result<(), Problem> {
        match &self.value {
            ExpressionValue::Literal(_) => Ok(()),
            ExpressionValue::Call(call) => check(call),
            ExpressionValue::List(items)
            | ExpressionValue::Variant(_, items)
            | ExpressionValue::Tuple(items) => {
                items.iter().try_for_each(|item| item.each_call(check))
            }
            ExpressionValue::Record(fields) | ExpressionValue::Map(fields) => fields
                .iter()
                .try_for_each(|(_, value)| value.each_call(check)),
        }
    }
}

pub(crate) struct Compiler<'a> {
    pub interpreter: &'a Interpreter,
    pub source: &'a str,
    pub opened: &'a BTreeSet<String>,
}

impl Compiler<'_> {
    pub fn element(&self, node: &Node) -> Result<Call, Problem> {
        let call = if self.is_scoped(node) {
            self.call(node.span)?
        } else {
            self.opened_call(node, None).unwrap_or_else(|| {
                Err(self.error(node.span, "expected a scoped call, such as `environment / list variables`, or a method from an opened tool"))
            })?
        };
        self.check_preconditions(&call)?;
        Ok(call)
    }

    /// Checks the `REQUIRES` of `call` and of each call in its arguments
    /// whose arguments are all written out. The rest are checked when their
    /// arguments have been computed, just before the call runs.
    fn check_preconditions(&self, call: &Call) -> Result<(), Problem> {
        for argument in &call.arguments {
            argument.each_call(&mut |nested| self.check_preconditions(nested))?;
        }
        let Some(values) = call
            .arguments
            .iter()
            .map(Argument::literal)
            .collect::<Option<Vec<_>>>()
        else {
            return Ok(());
        };
        let schema = self.interpreter.tools[&call.scope].schema();
        anyx_protocol::refinement::check_precondition(
            schema,
            schema.method_at(call.method),
            &values,
        )
        .map_err(|message| self.error(call.span, message))
    }

    fn opened_call(
        &self,
        node: &Node,
        target: Option<(&Schema, Option<&Type>)>,
    ) -> Option<Result<Call, Problem>> {
        let NodeKind::Call(call) = &node.kind else {
            return None;
        };
        let name = call.name_text();
        let mut fitting = Vec::new();
        let mut failure = None;
        for scope in self.opened {
            let schema = self.interpreter.tools[scope].schema();
            for index in schema.calls() {
                let method = schema.method_at(index);
                let Some(form) = method.shape.form(&name) else {
                    continue;
                };
                let written = method.shape.written_as(&form);
                if let Some((destination, expected)) = target {
                    let result = method.returns.as_ref().and_then(|value| {
                        transfer(schema, destination, &value.r#type, &mut BTreeSet::new())
                    });
                    match result {
                        Some(result) if expected.is_none_or(|ty| destination.fits(&result, ty)) => {
                        }
                        _ => {
                            failure.get_or_insert_with(|| {
                                self.error(node.span, format!(
                                "`{scope} / {name}` does not return a compatible argument type"
                            ))
                            });
                            continue;
                        }
                    }
                }
                match self.check_parameters(schema, &call.arguments, &written.parameters) {
                    Ok(arguments) => fitting.push(Call {
                        scope: scope.clone(),
                        method: index,
                        arguments: filled(schema, index, &form, arguments),
                        span: node.span,
                    }),
                    Err(error) => {
                        failure.get_or_insert(error);
                    }
                }
            }
        }
        match fitting.len() {
            0 => failure.map(Err),
            1 => fitting.pop().map(Ok),
            _ => {
                let candidates = fitting
                    .iter()
                    .map(|call| {
                        let schema = self.interpreter.tools[&call.scope].schema();
                        format!(
                            "`{} / {}`",
                            call.scope,
                            schema.typed_shape(&schema.method_at(call.method).shape)
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                Some(Err(self.error(
                    node.span,
                    format!(
                        "ambiguous call `{name}`; qualify the call with a tool scope: {candidates}"
                    ),
                )))
            }
        }
    }

    fn check_parameters(
        &self,
        schema: &Schema,
        nodes: &[Node],
        parameters: &[anyx_protocol::schema::Parameter],
    ) -> Result<Vec<Argument>, Problem> {
        nodes
            .iter()
            .zip(parameters)
            .map(
                |(node, parameter)| match self.argument(schema, node, None) {
                    Ok(argument) if argument.r#type == parameter.r#type => Ok(argument),
                    Ok(argument) => {
                        self.check(schema, node.span, argument, Some(&parameter.r#type))?;
                        self.argument(schema, node, Some(&parameter.r#type))
                    }
                    Err(_) => self.argument(schema, node, Some(&parameter.r#type)),
                },
            )
            .collect()
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
        // Each method a call of `name` may be, with the form it is written
        // in and its shape as written, which leaves out the optional parts
        // the call does.
        let methods: Vec<(usize, Form, Shape)> = (schema.calls())
            .filter_map(|index| {
                let shape = &schema.method_at(index).shape;
                let form = shape.form(&name)?;
                let written = shape.written_as(&form);
                Some((index, form, written))
            })
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
            .collect::<Result<Vec<_>, _>>();
        let arguments = match arguments {
            Ok(arguments) => arguments,
            Err(error) if self.opened.is_empty() => return Err(error),
            Err(error) => {
                let mut contextual = methods.iter().filter_map(|(method, form, written)| {
                    self.check_parameters(schema, &call.arguments, &written.parameters)
                        .ok()
                        .map(|arguments| Call {
                            scope: scope.text.clone(),
                            method: *method,
                            arguments: filled(schema, *method, form, arguments),
                            span,
                        })
                });
                match (contextual.next(), contextual.next()) {
                    (Some(call), None) => return Ok(call),
                    (None, _) => return Err(error),
                    _ => {
                        return Err(self.error(
                            node.span,
                            format!("ambiguous call `{name}` in `{}`", scope.text),
                        ));
                    }
                }
            }
        };
        let fitting: Vec<_> = methods
            .iter()
            .filter(|(_, _, written)| {
                let parameters = &written.parameters;
                parameters.len() == arguments.len()
                    && parameters
                        .iter()
                        .zip(&arguments)
                        .all(|(parameter, argument)| fits(schema, argument, &parameter.r#type))
            })
            .collect();
        let [(method, form, written)] = fitting.as_slice() else {
            let mismatch = methods
                .iter()
                .find_map(|(_, _, written)| {
                    written
                        .parameters
                        .iter()
                        .zip(&arguments)
                        .enumerate()
                        .find(|(_, (parameter, argument))| {
                            !fits(schema, argument, &parameter.r#type)
                        })
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
                .map(|(_, _, written)| schema.typed_shape(written))
                .collect::<Vec<_>>()
                .join(", ");
            // A result that fits a method once its refinements are left
            // out: say what its type would have to promise.
            let promise = methods
                .iter()
                .flat_map(|(_, _, written)| written.parameters.iter().zip(&arguments))
                .find(|(parameter, argument)| {
                    !fits(schema, argument, &parameter.r#type)
                        && schema.fits_literal(&argument.r#type, &parameter.r#type)
                })
                .map(|(parameter, argument)| {
                    format!(
                        "; a `{}` result doesn't promise the predicate of `{}`, so declare the method to return `{}`, or write the value itself",
                        schema.type_name(&argument.r#type),
                        schema.type_name(&parameter.r#type),
                        schema.type_name(&parameter.r#type)
                    )
                })
                .unwrap_or_default();
            return Err(self.error(
                mismatch,
                format!(
                    "`{name}` has argument types [{found}]; expected one of: {expected}{promise}"
                ),
            ));
        };
        // Re-read in the selected parameter types so literals are checked
        // against predicates before any call in the document can run.
        let arguments = call
            .arguments
            .iter()
            .zip(arguments)
            .zip(&written.parameters)
            .map(|((node, argument), parameter)| {
                if argument.r#type == parameter.r#type {
                    Ok(argument)
                } else {
                    self.argument(schema, node, Some(&parameter.r#type))
                }
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Call {
            scope: scope.text.clone(),
            method: *method,
            arguments: filled(schema, *method, form, arguments),
            span,
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
            return self.call_argument(schema, node.span, self.call(node.span)?, expected);
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
        if let Some(call) = self.opened_call(node, Some((schema, expected))) {
            return self.call_argument(schema, node.span, call?, expected);
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
            // A variant is marked with a backtick, `` `custom hue (x) ``,
            // and its slots hold expressions of the shell's.
            NodeKind::Call(marked) if marked.name_text() == "` _" => {
                let NodeKind::Call(call) = &marked.arguments[0].kind else {
                    return Err(literal_error);
                };
                let name = call.name_text();
                let candidates: Vec<_> = schema.types.iter().enumerate().filter_map(|(index, definition)| {
                    if definition.parameters.is_empty() && matches!(&definition.kind, TypeKind::Variants(variants) if variants.iter().any(|variant| variant.shape.form(&name).is_some())) {
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
                let TypeKind::Variants(variants) = kind.as_ref() else {
                    return Err(literal_error);
                };
                let Some((variant, form)) = variants
                    .iter()
                    .find_map(|variant| Some((variant, variant.shape.form(&name)?)))
                else {
                    return Err(literal_error);
                };
                let written = variant.shape.written_as(&form);
                if call.arguments.len() != written.parameters.len() {
                    return Err(literal_error);
                }
                let arguments = call
                    .arguments
                    .iter()
                    .zip(&written.parameters)
                    .map(|(node, parameter)| self.argument(schema, node, Some(&parameter.r#type)))
                    .collect::<Result<Vec<_>, _>>()?;
                // A variant holds its full form, each left-out part's
                // default among its arguments.
                let arguments = variant
                    .shape
                    .fill_with(&form, arguments, |index, value| Argument {
                        r#type: variant.shape.parameters[index].r#type.clone(),
                        value: ExpressionValue::Literal(value.clone()),
                    })
                    .expect("a checked schema has read every default");
                let shape = variant.shape.text.clone();
                Ok(Argument {
                    r#type,
                    value: ExpressionValue::Variant(shape, arguments),
                })
            }
            _ => Err(literal_error),
        }
    }

    fn call_argument(
        &self,
        schema: &Schema,
        span: Span,
        call: Call,
        expected: Option<&Type>,
    ) -> Result<Argument, Problem> {
        let origin = self.interpreter.tools[&call.scope].schema();
        let returns = origin
            .method_at(call.method)
            .returns
            .as_ref()
            .ok_or_else(|| {
                self.error(
                    span,
                    "a method that returns nothing cannot be used as an argument",
                )
            })?;
        let r#type = transfer(origin, schema, &returns.r#type, &mut BTreeSet::new())
            .ok_or_else(|| self.error(span, format!("the result type `{}` is not available with the same definition in this protocol", origin.type_name(&returns.r#type))))?;
        self.check(
            schema,
            span,
            Argument {
                r#type,
                value: ExpressionValue::Call(Box::new(call)),
            },
            expected,
        )
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
        // Each entry is a map's key and value, or a record's entry: a call
        // whose shape finds its field, as in `path _`, `hidden`, or a
        // variant, `` `by name ``.
        let mut fields: Vec<(String, &Node)> = Vec::new();
        let mut entry_calls: Vec<&anyx::Call> = Vec::new();
        let mut is_map = false;
        for entry in entries {
            let NodeKind::Call(call) = &entry.kind else {
                return Err(self.error(entry.span, "expected a record field or map entry"));
            };
            if call.name_text() == "_ _" && call.arguments.len() == 2 {
                let NodeKind::String(key) = &call.arguments[0].kind else {
                    return Err(self.error(entry.span, "expected a text map key"));
                };
                is_map = true;
                let key = key.value.clone();
                if fields.iter().any(|(previous, _)| *previous == key) {
                    return Err(self.error(entry.span, format!("duplicate key `{key}`")));
                }
                fields.push((key, &call.arguments[1]));
            } else {
                entry_calls.push(call);
            }
        }
        if is_map {
            if !entry_calls.is_empty() {
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
                let found: Vec<Option<usize>> = entries
                    .iter()
                    .zip(&entry_calls)
                    .map(|(entry, call)| record_entry(schema, record, entry, call))
                    .collect();
                (definition.parameters.is_empty()
                    && found.iter().all(Option::is_some)
                    && record.iter().enumerate().all(|(index, field)| {
                        field.default.is_some() || found.contains(&Some(index))
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
        let mut values: Vec<(String, Argument)> = Vec::new();
        for (entry, call) in entries.iter().zip(&entry_calls) {
            let (field, value) = match record_entry(schema, record, entry, call) {
                Some(index) => {
                    let field = &record[index];
                    let value = match field.form {
                        FieldForm::Checkbox => Argument {
                            r#type: field.r#type.clone(),
                            value: ExpressionValue::Literal(Value::Variant {
                                shape: "true".into(),
                                arguments: Vec::new(),
                            }),
                        },
                        // `` `by name ``: the entry is the value.
                        FieldForm::Unnamed => self.argument(schema, entry, Some(&field.r#type))?,
                        _ => match &field.r#type {
                            // A field with several slots holds a value for each.
                            Type::Tuple(slots) => Argument {
                                r#type: field.r#type.clone(),
                                value: ExpressionValue::Tuple(
                                    slots
                                        .iter()
                                        .zip(&call.arguments)
                                        .map(|(slot, argument)| {
                                            self.argument(schema, argument, Some(slot))
                                        })
                                        .collect::<Result<_, _>>()?,
                                ),
                            },
                            r#type => self.argument(schema, &call.arguments[0], Some(r#type))?,
                        },
                    };
                    (field, value)
                }
                None => {
                    return Err(
                        self.error(entry.span, format!("unknown field `{}`", call.name_text()))
                    );
                }
            };
            if values.iter().any(|(name, _)| *name == field.name) {
                return Err(self.error(entry.span, format!("duplicate field `{}`", field.name)));
            }
            values.push((field.name.clone(), value));
        }
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
                } else {
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
        if let Some(expected) = expected.filter(|expected| !fits(schema, &argument, expected)) {
            let promise = match schema.fits_literal(&argument.r#type, expected) {
                // Only a refinement stands between them.
                true => format!(
                    "; a `{}` result doesn't promise its predicate, so declare the method to return `{}`, or write the value itself",
                    schema.type_name(&argument.r#type),
                    schema.type_name(expected)
                ),
                false => String::new(),
            };
            return Err(self.error(
                span,
                format!(
                    "expression returns `{}`; expected `{}`{promise}",
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
/// Whether `argument` may be passed where `expected` is. A literal fits
/// a refinement of its own type, as `3` fits `Integer WHERE [0 <= _]`,
/// since its value is checked against the predicate; the result of a call
/// fits only when its type promises the predicate.
fn fits(schema: &Schema, argument: &Argument, expected: &Type) -> bool {
    match argument.literal() {
        Some(_) => schema.fits_literal(&argument.r#type, expected),
        None => schema.fits(&argument.r#type, expected),
    }
}

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
        Type::Refined(base, predicate) => Type::Refined(
            Box::new(transfer(origin, target, base, visited)?),
            predicate.clone(),
        ),
        Type::List(item) => Type::List(Box::new(transfer(origin, target, item, visited)?)),
        Type::Map(item) => Type::Map(Box::new(transfer(origin, target, item, visited)?)),
        Type::Tuple(slots) => Type::Tuple(
            slots
                .iter()
                .map(|slot| transfer(origin, target, slot, visited))
                .collect::<Option<Vec<_>>>()?,
        ),
        Type::Named(index) | Type::Generic(index, _) => {
            let definition = &origin.types[*index];
            let other = target.types.iter().position(|other| {
                other.name == definition.name && other.parameters == definition.parameters
            })?;
            if visited.insert((*index, other)) {
                let fits = match (&definition.kind, &target.types[other].kind) {
                    (
                        TypeKind::Distinct {
                            underlying: left, ..
                        },
                        TypeKind::Distinct {
                            underlying: right, ..
                        },
                    ) => transfer(origin, target, left, visited)? == *right,
                    (TypeKind::Record(left), TypeKind::Record(right)) => {
                        left.len() == right.len()
                            && left.iter().zip(right).all(|(left, right)| {
                                left.name == right.name
                                    && left.default.is_some() == right.default.is_some()
                                    && left.default == right.default
                                    && transfer(origin, target, &left.r#type, visited).as_ref()
                                        == Some(&right.r#type)
                            })
                    }
                    (TypeKind::Variants(left), TypeKind::Variants(right)) => {
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

/// The field of `record` a braces entry fills, by its shape, or for a
/// variant, `` `by name ``, by the variant's shape.
fn record_entry(
    schema: &Schema,
    record: &[Field],
    entry: &Node,
    call: &anyx::Call,
) -> Option<usize> {
    match call.name_text().as_str() {
        "` _" => schema.unnamed_field(record, &Written::of(entry)?),
        shape => schema.shaped_field(record, shape),
    }
}
