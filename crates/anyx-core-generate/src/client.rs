//! Calls a program that serves a schema, at run time, as a shell does: an
//! expression written against the schema is checked, sent as the JSON call
//! the generated server reads, and its answer read back as anyx.
//!
//! The expression is a constructor of an entry point's split family, such
//! as `find "x" with { hidden }` for `answer (response: Type) to (request:
//! Request returning (response))`. The kernel checks it and normalizes it,
//! so every `DEFAULT` it leaves out is a value by the time it is sent.

use crate::model::{Argument, Primitive, Schema, Ty};
use crate::{Error, camel_case};
use anyx::Span;
use anyx_core::{
    kernel::{self, Term},
    lowering::{self, RecordField},
    parser,
};
use std::collections::HashMap;

/// A schema read for calling the program that serves it.
pub struct Client {
    document: parser::Document,
    schema: Schema,
    records: HashMap<String, Vec<RecordField>>,
}

/// Why an expression isn't a call of the program.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CallError {
    /// The expression's name isn't one the schema declares, so it may be a
    /// call of another tool.
    Elsewhere,
    Invalid {
        span: Span,
        message: String,
    },
}

/// A checked call, ready to send.
pub struct Call {
    /// The request: a JSON list holding the one call.
    pub request: String,
    result: Ty,
}

impl Client {
    /// Parse, check and read the schema. `file` names it in errors.
    pub fn load(source: &str, file: &str) -> Result<Self, Error> {
        let schema = crate::load(source, file)?;
        let document = parser::parse_document(source)
            .document
            .expect("`load` parsed the schema");
        Ok(Self {
            records: lowering::record_fields(&document),
            document,
            schema,
        })
    }

    /// `source`, one expression written against the schema, as a call.
    pub fn call_written(&self, source: &str) -> Result<Call, CallError> {
        let parsed = parser::support::parse(source, |nodes| {
            let mut expressions = parser::support::document(nodes);
            let expression = expressions.one(parser::decode_expression, "an expression")?;
            expressions.finish()?;
            Ok(expression)
        });
        if let Some(error) = parsed.syntax_errors.first() {
            return Err(CallError::Invalid {
                span: error.span,
                message: error.message.clone(),
            });
        }
        let expression = parsed.document.map_err(|error| CallError::Invalid {
            span: error.span,
            message: error.message(),
        })?;
        self.call(&expression)
    }

    /// `expression` as a call of the entry point that answers it.
    pub fn call(&self, expression: &parser::Expression) -> Result<Call, CallError> {
        let span = expression.span();
        let invalid = |message: String| CallError::Invalid { span, message };
        let lowered = lowering::lower_expression(&self.document, expression).map_err(|error| {
            let head = match expression {
                parser::Expression::Call { name, .. } | parser::Expression::Name { name, .. } => {
                    Some(name.text.as_str())
                }
                _ => None,
            };
            if head.is_some_and(|head| error.message == format!("unknown name `{head}`")) {
                CallError::Elsewhere
            } else {
                CallError::Invalid {
                    span: error.span,
                    message: error.message,
                }
            }
        })?;
        let term = Term::try_from(&lowered).map_err(|error| invalid(error.to_string()))?;
        let checker = &self.schema.checker;
        let request_type = checker
            .synthesize(&term)
            .map_err(|error| invalid(error.to_string()))?;
        let request_type = checker
            .normalize(&request_type)
            .map_err(|error| invalid(error.to_string()))?;
        let value = checker
            .normalize(&term)
            .map_err(|error| invalid(error.to_string()))?;

        let family = match head(&request_type) {
            Some((name, _)) => name,
            None => return Err(invalid("this is a type, not a request".to_owned())),
        };
        let entry = self
            .schema
            .entries()
            .find(|entry| {
                entry
                    .split
                    .as_ref()
                    .is_some_and(|split| split.data == family)
            })
            .ok_or_else(|| {
                invalid(format!(
                    "no entry point answers a `{family}`, so this can't be sent"
                ))
            })?;
        let split = entry.split.as_ref().expect("entries are split");
        let (constructor, _) = head(&value)
            .ok_or_else(|| invalid("this request doesn't reduce to a constructor".to_owned()))?;
        let case = split
            .cases
            .iter()
            .find(|case| case.constructor == constructor)
            .ok_or_else(|| invalid(format!("`{constructor}` isn't a request")))?;
        let mut arguments = serde_json::Map::new();
        for (position, argument) in case.parameters.iter().enumerate() {
            let parameter = &entry.parameters[position];
            match argument {
                Argument::Split => {
                    arguments.insert(
                        camel_case(&parameter.name),
                        self.encode(&value).map_err(invalid)?,
                    );
                }
                Argument::Erased | Argument::Solved(_) => {}
                Argument::Collapsed { .. } | Argument::Plain { .. } => {
                    return Err(invalid(format!(
                        "`{}` also takes `{}`, which a call can't give yet",
                        entry.name, parameter.name
                    )));
                }
            }
        }
        let request = serde_json::json!([{ "function": entry.name, "arguments": arguments }]);
        Ok(Call {
            request: request.to_string(),
            result: case.result.clone(),
        })
    }

    /// The answer to `call` in `response`, the program's JSON answer, as an
    /// anyx value; or the message of a request it refused.
    pub fn answer(&self, call: &Call, response: &str) -> Result<String, String> {
        let answers: Vec<serde_json::Value> = serde_json::from_str(response)
            .map_err(|error| format!("the answer isn't a JSON list: {error}"))?;
        let [answer] = answers.as_slice() else {
            return Err(format!(
                "the answer holds {} results for one call",
                answers.len()
            ));
        };
        if answer.get("kind").and_then(|kind| kind.as_str()) == Some("invalid request") {
            return Err(answer
                .get("message")
                .and_then(|message| message.as_str())
                .unwrap_or("invalid request")
                .to_owned());
        }
        self.display(&call.result, answer)
    }

    /// A value as JSON, as the generated server reads it.
    fn encode(&self, value: &Term) -> Result<serde_json::Value, String> {
        use serde_json::Value as Json;
        Ok(match value {
            Term::Text(text) => Json::from(text.clone()),
            Term::Natural(number) => Json::from(
                u64::try_from(*number).map_err(|_| format!("{number} doesn't fit a `u64`"))?,
            ),
            Term::Integer(number) => Json::from(
                i64::try_from(*number).map_err(|_| format!("{number} doesn't fit an `i64`"))?,
            ),
            Term::List(items) => Json::Array(
                items
                    .iter()
                    .map(|item| self.encode(item))
                    .collect::<Result<_, _>>()?,
            ),
            _ => {
                let Some((name, arguments)) = head(value) else {
                    return Err("only a value can be sent".to_owned());
                };
                match name {
                    "true" => Json::Bool(true),
                    "false" => Json::Bool(false),
                    "done" | "nothing of _" | "empty list of _" => {
                        if name == "empty list of _" {
                            Json::Array(Vec::new())
                        } else {
                            Json::Null
                        }
                    }
                    "something of _ is _" => self.encode(&arguments[1])?,
                    kernel::LIST_CELL => {
                        let mut items = vec![self.encode(&arguments[1])?];
                        match self.encode(&arguments[2])? {
                            Json::Array(rest) => items.extend(rest),
                            _ => return Err("a list ends in a list".to_owned()),
                        }
                        Json::Array(items)
                    }
                    _ => {
                        let constructor = self.schema.constructor(name);
                        let data = self.schema.data(&constructor.data);
                        let mut object = serde_json::Map::new();
                        if !data.record {
                            object.insert("kind".to_owned(), Json::from(constructor.kind()));
                        }
                        for (position, binder) in constructor.binders.iter().enumerate() {
                            if constructor.erased(position) {
                                continue;
                            }
                            object.insert(
                                camel_case(&binder.name),
                                self.encode(&arguments[position])?,
                            );
                        }
                        Json::Object(object)
                    }
                }
            }
        })
    }

    /// Whether `json`, of type `ty`, is written as words, which an argument
    /// puts in parentheses: `answered (true)`, but `answered [1]`.
    fn is_words(&self, ty: &Ty, json: &serde_json::Value) -> bool {
        match ty {
            Ty::Data { name, .. } => !self.schema.data(name).record,
            Ty::Optional(item) => json.is_null() || self.is_words(item, json),
            Ty::Primitive(Primitive::Nothing | Primitive::Boolean) => true,
            _ => false,
        }
    }

    /// A JSON value of type `ty` written as anyx: a record as `{ }` with
    /// its fields as written, any other value as its constructor's words.
    fn display(&self, ty: &Ty, json: &serde_json::Value) -> Result<String, String> {
        use serde_json::Value as Json;
        let unexpected = || format!("the answer `{json}` doesn't fit its type");
        Ok(match ty {
            Ty::Primitive(primitive) => match (primitive, json) {
                (Primitive::Text, Json::String(text)) => {
                    serde_json::to_string(text).expect("a string serializes")
                }
                (
                    Primitive::Integer
                    | Primitive::Natural
                    | Primitive::Instant
                    | Primitive::Duration,
                    Json::Number(number),
                ) => number.to_string(),
                (Primitive::Boolean, Json::Bool(boolean)) => boolean.to_string(),
                (Primitive::Nothing, Json::Null) => "done".to_owned(),
                _ => return Err(unexpected()),
            },
            Ty::List(item) => {
                let Json::Array(items) = json else {
                    return Err(unexpected());
                };
                let items = items
                    .iter()
                    .map(|value| self.display(item, value))
                    .collect::<Result<Vec<_>, _>>()?;
                format!("[{}]", items.join(", "))
            }
            Ty::Optional(item) => match json {
                Json::Null => "nothing".to_owned(),
                value => self.display(item, value)?,
            },
            Ty::Data { name, arguments } => {
                let data = self.schema.data(name);
                let generics: HashMap<&str, &Ty> = data
                    .generics
                    .iter()
                    .map(String::as_str)
                    .zip(arguments)
                    .collect();
                let Json::Object(object) = json else {
                    return Err(unexpected());
                };
                let constructor = if data.record {
                    &data.constructors[0]
                } else {
                    let kind = object
                        .get("kind")
                        .and_then(|kind| kind.as_str())
                        .ok_or_else(unexpected)?;
                    data.constructors
                        .iter()
                        .find(|constructor| constructor.kind() == kind)
                        .ok_or_else(unexpected)?
                };
                let mut values = HashMap::new();
                for (field, field_ty) in constructor.fields() {
                    let value = object.get(&camel_case(field)).ok_or_else(unexpected)?;
                    let field_ty = substitute(field_ty, &generics);
                    let written = self.display(&field_ty, value)?;
                    let argument = if self.is_words(&field_ty, value) {
                        format!("({written})")
                    } else {
                        written.clone()
                    };
                    values.insert(field.to_owned(), (written, argument));
                }
                if data.record {
                    let fields = self.records.get(&data.name).ok_or_else(unexpected)?;
                    let entries = fields
                        .iter()
                        .map(|field| match &field.name {
                            None => Ok(values[&field.binders[0]].0.clone()),
                            Some(name) => fill(
                                name,
                                field
                                    .binders
                                    .iter()
                                    .map(|binder| Some(values[binder].1.as_str())),
                            ),
                        })
                        .collect::<Result<Vec<_>, String>>()?;
                    format!("{{ {} }}", entries.join(", "))
                } else {
                    fill(
                        &constructor.name,
                        constructor.binders.iter().map(|binder| {
                            binder.ty.as_ref().map(|_| values[&binder.name].1.as_str())
                        }),
                    )?
                }
            }
            Ty::Variable(_) | Ty::Opaque(_) | Ty::Erased => json.to_string(),
        })
    }
}

/// The constructor or function a value applies, and its arguments.
fn head(term: &Term) -> Option<(&str, &[Term])> {
    match term {
        Term::Global(global) => Some((global.name.as_str(), &[])),
        Term::Apply {
            function,
            arguments,
        } => match function.as_ref() {
            Term::Global(global) => Some((global.name.as_str(), arguments.as_slice())),
            _ => None,
        },
        _ => None,
    }
}

/// `name` with each `_` filled by its value, as an argument. An erased slot
/// (`None`) is left out with the words up to the next slot, as `kind` does.
fn fill<'value>(
    name: &str,
    values: impl IntoIterator<Item = Option<&'value str>>,
) -> Result<String, String> {
    let mut values = values.into_iter();
    let mut words = Vec::new();
    let mut skipping = false;
    for word in name.split_whitespace() {
        if word == "_" {
            match values.next().flatten() {
                Some(value) => {
                    skipping = false;
                    words.push(value.to_owned());
                }
                None => skipping = true,
            }
        } else if !skipping {
            words.push(word.to_owned());
        }
    }
    Ok(words.join(" "))
}

fn substitute(ty: &Ty, generics: &HashMap<&str, &Ty>) -> Ty {
    match ty {
        Ty::Variable(name) => generics
            .get(name.as_str())
            .map_or_else(|| ty.clone(), |ty| (*ty).clone()),
        Ty::List(item) => Ty::List(Box::new(substitute(item, generics))),
        Ty::Optional(item) => Ty::Optional(Box::new(substitute(item, generics))),
        Ty::Data { name, arguments } => Ty::Data {
            name: name.clone(),
            arguments: arguments
                .iter()
                .map(|argument| substitute(argument, generics))
                .collect(),
        },
        Ty::Primitive(_) | Ty::Opaque(_) | Ty::Erased => ty.clone(),
    }
}
