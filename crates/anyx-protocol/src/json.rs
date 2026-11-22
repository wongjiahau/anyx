//! Values as JSON, for a TypeScript client that calls the codec through wasm.
//!
//! Names are camelCase: the field `file summary` is `fileSummary`. A choice's
//! variant or error is an object whose `kind` is its words, next to its
//! parameters: `{ "kind": "conflictAtRevision", "revision": 4 }`.

use std::collections::BTreeMap;

use serde_json::{Map, Value as Json, json};

use crate::codec::{Call, Outcome, Response, decode_request, encode_request};
use crate::response::{decode_response, encode_response};
use crate::schema::{
    Field, JsonForm, Schema, Shape, ShapePart, Type, TypeKind, json_tag, slot_types,
};
use crate::value::Value;

pub use crate::schema::camel_case;

/// Writes one call of the method `full_name`, such as `files/read _` or
/// `SCHEMA`, as a request. An overload is named with its parameters'
/// types, `tag (LIST Text)`, which its full name alone leaves open.
/// `arguments` is a JSON array with one value per parameter.
pub fn encode_request_json(
    schema: &Schema,
    full_name: &str,
    arguments: &str,
) -> Result<String, String> {
    let arguments: Json = serde_json::from_str(arguments).map_err(|error| error.to_string())?;
    let call = call_from_json(schema, full_name, &arguments)?;
    encode_request(schema, &[call]).map_err(|error| error.message)
}

/// Writes several calls as one request, from a JSON array with one
/// `{ "method": …, "arguments": […] }` per call, in the form
/// [`decode_request_json`] reads them as. The server runs them in order.
pub fn encode_requests_json(schema: &Schema, calls: &str) -> Result<String, String> {
    let calls: Vec<Json> = serde_json::from_str(calls).map_err(|error| error.to_string())?;
    let calls = calls
        .iter()
        .map(|call| {
            let method = call
                .get("method")
                .and_then(Json::as_str)
                .ok_or_else(|| format!("found the call {call}; expected its \"method\""))?;
            call_from_json(schema, method, call.get("arguments").unwrap_or(&Json::Null))
        })
        .collect::<Result<Vec<_>, _>>()?;
    encode_request(schema, &calls).map_err(|error| error.message)
}

fn call_from_json(schema: &Schema, full_name: &str, arguments: &Json) -> Result<Call, String> {
    let index = method_index(schema, full_name)?;
    let method = schema.method_at(index);
    let Json::Array(arguments) = arguments else {
        return Err(format!(
            "found the arguments {arguments} for `{full_name}`; expected an array"
        ));
    };
    if arguments.len() != method.shape.parameters.len() {
        return Err(format!(
            "found {} arguments for `{full_name}`; expected {}",
            arguments.len(),
            method.shape.parameters.len()
        ));
    }
    let arguments = method
        .shape
        .parameters
        .iter()
        .zip(arguments)
        .map(|(parameter, json)| {
            from_json(schema, &parameter.r#type, json)
                .map_err(|message| format!("in `{}`: {message}", parameter.name))
        })
        .collect::<Result<_, _>>()?;
    Ok(Call {
        method: index,
        arguments,
    })
}

/// Reads a request, as a JSON array with one `{ "method": …, "arguments":
/// […] }` per call, `method` being its full name such as `files/read _`,
/// or for an overload its name with its parameters' types, `tag (LIST:
/// Text)`.
/// For a tool that watches requests go by, like a test faking a server.
pub fn decode_request_json(schema: &Schema, text: &str) -> Result<String, String> {
    let calls = decode_request(schema, text).map_err(|problem| problem.to_string())?;
    let calls: Vec<Json> = calls
        .iter()
        .map(|call| {
            let method = schema.method_at(call.method);
            json!({
                "method": schema.call_name(method),
                "arguments": method
                    .shape
                    .parameters
                    .iter()
                    .zip(&call.arguments)
                    .map(|(parameter, value)| to_json(schema, &parameter.r#type, value))
                    .collect::<Vec<_>>(),
            })
        })
        .collect();
    Ok(Json::Array(calls).to_string())
}

/// Writes the response to `request`, from a JSON array with one outcome per
/// call in the form [`decode_response_json`] reads them as. For a test that
/// fakes a server in front of the TypeScript client.
pub fn encode_response_json(
    schema: &Schema,
    request: &str,
    outcomes: &str,
) -> Result<String, String> {
    let calls = decode_request(schema, request).map_err(|problem| problem.to_string())?;
    let outcomes: Vec<Json> = serde_json::from_str(outcomes).map_err(|error| error.to_string())?;
    if outcomes.len() != calls.len() {
        return Err(format!(
            "found {} outcomes for {} calls",
            outcomes.len(),
            calls.len()
        ));
    }
    let outcomes = calls
        .iter()
        .zip(&outcomes)
        .map(|(call, json)| outcome_from_json(schema, call.method, json))
        .collect::<Result<_, _>>()?;
    encode_response(schema, &calls, &Response::Outcomes(outcomes)).map_err(|error| error.message)
}

fn outcome_from_json(schema: &Schema, index: usize, json: &Json) -> Result<Outcome, String> {
    let method = schema.method_at(index);
    match json.get("kind").and_then(Json::as_str) {
        Some("value") => match &method.returns {
            Some(returns) => {
                let value = json.get("value").unwrap_or(&Json::Null);
                from_json(schema, &returns.r#type, value).map(Outcome::Value)
            }
            None => Ok(Outcome::Value(Value::Nothing)),
        },
        Some("error") => {
            let shapes: Vec<&Shape> = schema
                .errors_of(method)
                .iter()
                .map(|error| &error.shape)
                .collect();
            match json.get("error") {
                Some(Json::Object(error)) => {
                    variant_from_json(schema, &shapes, error).map(Outcome::Error)
                }
                _ => Err("found an error outcome without an \"error\" object".to_string()),
            }
        }
        Some("notRun") => Ok(Outcome::NotRun),
        Some("internalError") => Ok(Outcome::InternalError),
        _ => Err(format!(
            "found the outcome {json}; expected its \"kind\": \"value\", \"error\", \"notRun\" or \"internalError\""
        )),
    }
}

/// Reads the response to one call of the method `full_name`, as JSON:
/// `{ "kind": "value", "value": … }`, `{ "kind": "error", "error": … }`,
/// `{ "kind": "notRun" }`,
/// `{ "kind": "internalError" }`, or
/// `{ "kind": "invalidRequest", "line": …, "column": …, "message": … }`.
pub fn decode_response_json(
    schema: &Schema,
    full_name: &str,
    text: &str,
) -> Result<String, String> {
    let index = method_index(schema, full_name)?;
    let outcomes = decode_outcomes(schema, &[index], text)?;
    Ok(outcomes
        .into_iter()
        .next()
        .unwrap_or(json!({ "kind": "internalError" }))
        .to_string())
}

/// Reads the response to a request of several calls, one of each method in
/// `full_names` (a JSON array of full names) in order, as a JSON array with
/// one outcome per call in the form [`decode_response_json`] gives. A
/// response the server couldn't read the request of gives that
/// `invalidRequest` for every call.
pub fn decode_responses_json(
    schema: &Schema,
    full_names: &str,
    text: &str,
) -> Result<String, String> {
    let full_names: Vec<String> =
        serde_json::from_str(full_names).map_err(|error| error.to_string())?;
    let methods = full_names
        .iter()
        .map(|full_name| method_index(schema, full_name))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Json::Array(decode_outcomes(schema, &methods, text)?).to_string())
}

fn decode_outcomes(schema: &Schema, methods: &[usize], text: &str) -> Result<Vec<Json>, String> {
    let response = decode_response(schema, methods, text).map_err(|problem| problem.to_string())?;
    Ok(match response {
        Response::InvalidRequest(problem) => {
            let invalid = json!({
                "kind": "invalidRequest",
                "line": problem.line,
                "column": problem.column,
                "message": problem.message,
            });
            methods.iter().map(|_| invalid.clone()).collect()
        }
        Response::Outcomes(outcomes) => {
            let mut outcomes = outcomes.into_iter();
            methods
                .iter()
                .map(|index| outcome_to_json(schema, *index, outcomes.next()))
                .collect()
        }
    })
}

fn outcome_to_json(schema: &Schema, index: usize, outcome: Option<Outcome>) -> Json {
    let method = schema.method_at(index);
    match outcome {
        Some(Outcome::Value(value)) => match &method.returns {
            Some(returns) => json!({
                "kind": "value",
                "value": to_json(schema, &returns.r#type, &value),
            }),
            None => json!({ "kind": "value" }),
        },
        Some(Outcome::Error(Value::Variant { shape, arguments })) => {
            let shapes: Vec<&Shape> = schema
                .errors_of(method)
                .iter()
                .map(|error| &error.shape)
                .collect();
            json!({ "kind": "error", "error": variant_to_json(schema, &shapes, &shape, &arguments) })
        }
        Some(Outcome::NotRun) => json!({ "kind": "notRun" }),
        Some(Outcome::InternalError | Outcome::Error(_)) | None => {
            json!({ "kind": "internalError" })
        }
    }
}

/// The method that `name` calls: by its full name, `tag _`, or by its
/// typed name, `tag (LIST Text)`, which an overload needs.
fn method_index(schema: &Schema, name: &str) -> Result<usize, String> {
    let methods = schema.calls();
    if let Some(index) = methods
        .clone()
        .find(|index| schema.typed_name(schema.method_at(*index)) == name)
    {
        return Ok(index);
    }
    let named: Vec<usize> = methods
        .filter(|index| schema.method_at(*index).full_name() == name)
        .collect();
    match named[..] {
        [index] => Ok(index),
        [] => Err(format!(
            "found the method `{name}`; expected a method of the schema"
        )),
        _ => Err(format!(
            "found the method `{name}`, which has overloads; expected one of them by its name with its parameters' types: {}",
            named
                .iter()
                .map(|index| format!("`{}`", schema.typed_name(schema.method_at(*index))))
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}

/// `value` as JSON.
pub fn to_json(schema: &Schema, r#type: &Type, value: &Value) -> Json {
    let r#type = schema.underlying(r#type);
    if let Type::Union(_) = r#type
        && let Some(member) = schema.member_holding(r#type, value)
    {
        return to_json(schema, member, value);
    }
    match value {
        Value::Text(text) => Json::String(text.clone()),
        Value::Bytes(bytes) => Json::String(crate::bytes::encode(bytes)),
        Value::Integer(integer) => json!(integer),
        Value::Float(number) => json!(number),
        Value::Instant(instant) => {
            crate::time::format_instant(*instant).map_or(Json::Null, Json::String)
        }
        Value::Duration(duration) => u64::try_from(duration.as_millis())
            .map_or(Json::Null, |milliseconds| json!(milliseconds)),
        Value::Nothing => Json::Null,
        Value::List(items) => {
            let item = match r#type {
                Type::List(item) => item.as_ref(),
                other => other,
            };
            Json::Array(
                items
                    .iter()
                    .map(|value| to_json(schema, item, value))
                    .collect(),
            )
        }
        Value::Tuple(values) => slots_to_json(schema, r#type, values),
        Value::Map(entries) => {
            let value_type = match r#type {
                Type::Map(value) => value.as_ref(),
                other => other,
            };
            Json::Object(
                entries
                    .iter()
                    .map(|(key, value)| (key.clone(), to_json(schema, value_type, value)))
                    .collect(),
            )
        }
        Value::Record(values) => {
            let kind = schema.instance(r#type).map(|(_, kind)| kind);
            let fields = match kind.as_deref() {
                Some(TypeKind::Record(fields)) => &fields[..],
                _ => &[],
            };
            Json::Object(fields_to_json(schema, fields, values))
        }
        Value::Variant { shape, arguments } => {
            if let Type::Variant(own) = r#type {
                return variant_to_json(schema, &[own.as_ref()], shape, arguments);
            }
            let kind = schema.instance(r#type).map(|(_, kind)| kind);
            if let Some(TypeKind::Distinct {
                constructor,
                underlying,
            }) = kind.as_deref()
            {
                return named_to_json(schema, constructor, underlying, arguments);
            }
            let variants = match kind.as_deref() {
                Some(TypeKind::Variants(variants)) => {
                    variants.iter().map(|variant| &variant.shape).collect()
                }
                _ => Vec::new(),
            };
            variant_to_json(schema, &variants, shape, arguments)
        }
    }
}

fn slots_to_json(schema: &Schema, r#type: &Type, values: &[Value]) -> Json {
    let Type::Tuple(types, names) = r#type else {
        return Json::Object(Map::new());
    };
    if names.is_empty() {
        return Json::Array(Vec::new());
    }
    Json::Object(
        names
            .iter()
            .zip(types)
            .zip(values)
            .map(|((name, slot), value)| (camel_case(name), to_json(schema, slot, value)))
            .collect(),
    )
}

fn slots_from_json(
    schema: &Schema,
    types: &[Type],
    names: &[String],
    json: &Json,
) -> Result<Vec<Value>, String> {
    if names.is_empty() && json.as_array().is_some_and(Vec::is_empty) {
        return Ok(Vec::new());
    }
    let object = json
        .as_object()
        .filter(|object| !names.is_empty() && object.len() == names.len())
        .ok_or_else(|| {
            format!(
                "found {json}; expected {}",
                if names.is_empty() {
                    "an empty array".to_string()
                } else {
                    format!(
                        "an object with slots {}",
                        names
                            .iter()
                            .map(|name| camel_case(name))
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                }
            )
        })?;
    names
        .iter()
        .zip(types)
        .map(|(name, slot)| {
            let key = camel_case(name);
            let item = object
                .get(&key)
                .ok_or_else(|| format!("found no slot {key:?}"))?;
            from_json(schema, slot, item).map_err(|message| format!("in {key:?}: {message}"))
        })
        .collect()
}

/// A named type's value under its tag.
fn named_to_json(
    schema: &Schema,
    constructor: &[ShapePart],
    underlying: &Type,
    arguments: &[Value],
) -> Json {
    let held = match (slot_types(constructor, underlying).as_slice(), arguments) {
        ([slot], [value]) => to_json(schema, slot, value),
        _ => slots_to_json(schema, underlying, arguments),
    };
    json!({ json_tag(constructor): held })
}

/// Reads a named type's value from under its tag.
fn named_from_json(
    schema: &Schema,
    constructor: &[ShapePart],
    underlying: &Type,
    json: &Json,
) -> Result<Value, String> {
    let tag = json_tag(constructor);
    let held = match json {
        Json::Object(object) if object.len() == 1 => object.get(&tag),
        _ => None,
    };
    let Some(held) = held else {
        return Err(format!(
            "found {json}; expected an object whose one key is \"{tag}\", the tag of `{}`",
            ShapePart::words(constructor)
        ));
    };
    let arguments = match slot_types(constructor, underlying).as_slice() {
        [slot] => vec![from_json(schema, slot, held)?],
        _ => {
            let Type::Tuple(types, names) = underlying else {
                unreachable!()
            };
            slots_from_json(schema, types, names, held)
                .map_err(|message| format!("under {tag:?}: {message}"))?
        }
    };
    Ok(Value::Variant {
        shape: ShapePart::text(constructor),
        arguments,
    })
}

/// A variant or an error as `{ "kind": "conflictAtRevision", "revision": 4 }`.
pub fn variant_to_json(
    schema: &Schema,
    shapes: &[&Shape],
    shape: &str,
    arguments: &[Value],
) -> Json {
    let mut object = Map::new();
    let Some(shape) = shapes.iter().find(|known| known.text == shape) else {
        return Json::Object(object);
    };
    object.insert("kind".to_string(), Json::String(camel_case(&shape.words())));
    for (parameter, value) in shape.parameters.iter().zip(arguments) {
        object.insert(
            camel_case(&parameter.name),
            to_json(schema, &parameter.r#type, value),
        );
    }
    Json::Object(object)
}

fn fields_to_json(
    schema: &Schema,
    fields: &[Field],
    values: &BTreeMap<String, Value>,
) -> Map<String, Json> {
    fields
        .iter()
        .filter_map(|field| {
            let value = values.get(&field.name)?;
            Some((
                camel_case(&field.name),
                to_json(schema, &field.r#type, value),
            ))
        })
        .collect()
}

/// Reads JSON as a value of `type`, checking it against the schema.
pub fn from_json(schema: &Schema, r#type: &Type, json: &Json) -> Result<Value, String> {
    let value = from_json_raw(schema, r#type, json)?;
    crate::refinement::validate(schema, r#type, &value)?;
    Ok(value)
}

fn from_json_raw(schema: &Schema, r#type: &Type, json: &Json) -> Result<Value, String> {
    let r#type = schema.underlying(r#type);
    let mismatch = |expected: &str| Err(format!("found {json}; expected {expected}"));
    match r#type {
        Type::Text => match json {
            Json::String(text) => Ok(Value::Text(text.clone())),
            _ => mismatch("a string"),
        },
        Type::Bytes => match json {
            Json::String(text) => match crate::bytes::decode(text) {
                Some(bytes) => Ok(Value::Bytes(bytes)),
                None => mismatch("base64 in a string"),
            },
            _ => mismatch("base64 in a string"),
        },
        Type::Integer => match json.as_i64() {
            Some(integer) => Ok(Value::Integer(integer)),
            None => mismatch("a whole number"),
        },
        Type::Float => match json.as_f64() {
            Some(number) => Ok(Value::Float(number)),
            None => mismatch("a number"),
        },
        Type::Instant => match json.as_str().and_then(crate::time::parse_instant) {
            Some(instant) => Ok(Value::Instant(instant)),
            None => mismatch("an instant in UTC in a string, as in \"2026-11-14T06:00:08.000Z\""),
        },
        Type::Duration => match json.as_u64() {
            Some(milliseconds) => Ok(Value::Duration(std::time::Duration::from_millis(
                milliseconds,
            ))),
            None => mismatch("a duration in whole milliseconds"),
        },
        Type::List(item) => match json {
            Json::Array(items) => items
                .iter()
                .map(|json| from_json(schema, item, json))
                .collect::<Result<_, _>>()
                .map(Value::List),
            _ => mismatch("an array"),
        },
        Type::Tuple(types, names) => slots_from_json(schema, types, names, json).map(Value::Tuple),
        Type::Map(value_type) => match json {
            Json::Object(object) => object
                .iter()
                .map(|(key, json)| {
                    from_json(schema, value_type, json).map(|value| (key.clone(), value))
                })
                .collect::<Result<_, _>>()
                .map(Value::Map),
            _ => mismatch("an object"),
        },
        Type::Named(_) | Type::Generic(..) => {
            let Some((definition, kind)) = schema.instance(r#type) else {
                unreachable!("a named or generic type is a record or a choice underneath")
            };
            if let TypeKind::Distinct {
                constructor,
                underlying,
            } = kind.as_ref()
            {
                return named_from_json(schema, constructor, underlying, json);
            }
            let Json::Object(object) = json else {
                return mismatch(&format!("an object for `{}`", definition.shape()));
            };
            match kind.as_ref() {
                TypeKind::Record(fields) => {
                    fields_from_json(schema, fields, object).map(Value::Record)
                }
                TypeKind::Variants(variants) => {
                    let shapes: Vec<&Shape> =
                        variants.iter().map(|variant| &variant.shape).collect();
                    variant_from_json(schema, &shapes, object)
                }
                TypeKind::Distinct { .. } => unreachable!("read above"),
            }
        }
        Type::Union(_) => {
            let found = match json {
                Json::Number(_) => JsonForm::Number,
                Json::String(_) => JsonForm::String,
                Json::Array(_) => JsonForm::Array,
                Json::Object(object) => match object.get("kind").and_then(Json::as_str) {
                    Some(kind) => JsonForm::Kind(kind.to_string()),
                    None => JsonForm::Object,
                },
                Json::Null | Json::Bool(_) => JsonForm::Object,
            };
            // An object of one key may be a named type's, under its tag.
            let tag = match json {
                Json::Object(object) if object.len() == 1 => {
                    object.keys().next().map(|key| JsonForm::Tag(key.clone()))
                }
                _ => None,
            };
            let holds = |form: &JsonForm| {
                r#type.members().into_iter().find(|member| {
                    schema
                        .json_forms(member)
                        .is_some_and(|forms| forms.contains(form))
                })
            };
            let member = tag.as_ref().and_then(holds).or_else(|| holds(&found));
            match member {
                Some(member) => from_json(schema, member, json),
                None => mismatch(&format!(
                    "a value of one of {}",
                    r#type
                        .members()
                        .iter()
                        .map(|member| crate::schema::code(&schema.type_name(member)))
                        .collect::<Vec<_>>()
                        .join(", ")
                )),
            }
        }
        Type::Variant(shape) => match json {
            Json::Object(object) => variant_from_json(schema, &[shape.as_ref()], object),
            _ => mismatch(&format!("an object for `{}`", schema.shape_name(shape))),
        },
        // The checker replaces each parameter by its argument wherever a
        // value is read.
        Type::Refined(..) => unreachable!("underlying removes refinements"),
        Type::Parameter(name) => mismatch(&format!(
            "a value of a type in place of the type parameter `{name}`"
        )),
    }
}

/// Reads `{ "kind": …, … }` as one of `shapes`.
pub fn variant_from_json(
    schema: &Schema,
    shapes: &[&Shape],
    object: &Map<String, Json>,
) -> Result<Value, String> {
    let kind = object
        .get("kind")
        .and_then(Json::as_str)
        .unwrap_or_default();
    let Some(shape) = shapes
        .iter()
        .find(|shape| camel_case(&shape.words()) == kind)
    else {
        let kinds: Vec<String> = shapes
            .iter()
            .map(|shape| format!("\"{}\"", camel_case(&shape.words())))
            .collect();
        return Err(format!(
            "found the kind \"{kind}\"; expected one of {}",
            kinds.join(", ")
        ));
    };
    if let Some(unknown) = object.keys().find(|key| {
        *key != "kind"
            && !shape
                .parameters
                .iter()
                .any(|parameter| camel_case(&parameter.name) == **key)
    }) {
        return Err(format!(
            "found the field \"{unknown}\"; expected only the variant's parameters"
        ));
    }
    let arguments = shape
        .parameters
        .iter()
        .map(|parameter| {
            let name = camel_case(&parameter.name);
            let json = object
                .get(&name)
                .ok_or_else(|| format!("found no field \"{name}\""))?;
            from_json(schema, &parameter.r#type, json)
                .map_err(|message| format!("in \"{name}\": {message}"))
        })
        .collect::<Result<_, _>>()?;
    Ok(Value::Variant {
        shape: shape.text.clone(),
        arguments,
    })
}

fn fields_from_json(
    schema: &Schema,
    fields: &[Field],
    object: &Map<String, Json>,
) -> Result<BTreeMap<String, Value>, String> {
    if let Some(unknown) = object
        .keys()
        .find(|key| !fields.iter().any(|field| camel_case(&field.name) == **key))
    {
        return Err(format!(
            "found the field \"{unknown}\"; expected only the schema's fields"
        ));
    }
    let mut values = BTreeMap::new();
    for field in fields {
        match object.get(&camel_case(&field.name)) {
            None if field.default.is_some() => {
                if let Some(default) = &field.default {
                    values.insert(field.name.clone(), default.clone());
                }
            }
            None => return Err(format!("found no field \"{}\"", camel_case(&field.name))),
            Some(json) => {
                let value = from_json(schema, &field.r#type, json)
                    .map_err(|message| format!("in \"{}\": {message}", camel_case(&field.name)))?;
                values.insert(field.name.clone(), value);
            }
        }
    }
    Ok(values)
}
