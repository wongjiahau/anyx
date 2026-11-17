//! A small example tool backed by the interpreter's own environment map.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use anyx_protocol::{Call, FieldsBuilder, Outcome, Schema, Value};

use crate::Tool;

pub const SCHEMA: &str = include_str!("../environment.protocol.anyx");

pub fn schema() -> &'static Schema {
    static SCHEMA_VALUE: OnceLock<Schema> = OnceLock::new();
    SCHEMA_VALUE
        .get_or_init(|| Schema::parse(SCHEMA).expect("the bundled environment protocol is valid"))
}

/// Names are case-sensitive, including on Windows. All names and values are
/// Unicode. This map never mutates Rust's process-global environment.
#[derive(Default)]
pub struct Environment {
    variables: BTreeMap<String, String>,
}

impl Environment {
    pub fn new() -> Self {
        Self::default()
    }

    /// Takes one snapshot. An inherited non-Unicode name or value is reported
    /// rather than silently dropped or replaced with lossy text.
    pub fn from_process() -> Result<Self, String> {
        let mut environment = Self::new();
        for (name, value) in std::env::vars_os() {
            let name = name
                .into_string()
                .map_err(|_| "an inherited environment variable name is not Unicode")?;
            let value = value
                .into_string()
                .map_err(|_| "an inherited environment variable value is not Unicode")?;
            if !valid_name(&name) {
                return Err("an inherited environment variable has an invalid name".into());
            }
            environment.variables.insert(name, value);
        }
        Ok(environment)
    }
}

impl Tool for Environment {
    fn schema(&self) -> &Schema {
        schema()
    }

    fn invoke(&mut self, call: &Call) -> Outcome {
        let method = &schema().method_at(call.method).shape.text;
        if method == "list variables" {
            return Outcome::Value(Value::List(
                self.variables
                    .iter()
                    .map(|(name, value)| variable(name.clone(), value.clone()))
                    .collect(),
            ));
        }
        let mut arguments = call.arguments.clone().into_iter();
        let name: String = anyx_protocol::server::argument(&mut arguments);
        if !valid_name(&name) {
            return error("invalid variable name");
        }
        match method.as_str() {
            "read variable _" => match self.variables.get(&name) {
                Some(value) => Outcome::Value(Value::Text(value.clone())),
                None => error("variable does not exist"),
            },
            "set variable _ to _" => {
                let value: String = anyx_protocol::server::argument(&mut arguments);
                if value.contains('\0') {
                    return error("invalid variable value");
                }
                self.variables.insert(name.clone(), value.clone());
                Outcome::Value(variable(name, value))
            }
            "remove variable _" => Outcome::Value(anyx_protocol::IntoValue::into_value(
                self.variables.remove(&name).is_some(),
            )),
            _ => unreachable!("the interpreter checked the method against the schema"),
        }
    }
}

fn valid_name(name: &str) -> bool {
    !name.is_empty() && !name.contains(['=', '\0'])
}

fn variable(name: String, value: String) -> Value {
    Value::Record(
        FieldsBuilder::new()
            .field("name", name)
            .field("value", value)
            .build(),
    )
}

fn error(shape: &str) -> Outcome {
    Outcome::Error(Value::Variant {
        shape: shape.into(),
        arguments: vec![],
    })
}
