//! The shell operations on the protocol prelude's Boolean choice.

use std::sync::OnceLock;

use anyx_protocol::prelude::BooleanOperation;
use anyx_protocol::{Call, IntoValue, Outcome, Schema, server::argument};

use crate::Tool;

pub struct Boolean;

impl Tool for Boolean {
    fn schema(&self) -> &Schema {
        static SCHEMA: OnceLock<Schema> = OnceLock::new();
        SCHEMA.get_or_init(|| {
            Schema::parse(include_str!("../boolean.protocol.anyx"))
                .expect("the bundled Boolean protocol is valid")
        })
    }

    fn invoke(&mut self, call: &Call) -> Outcome {
        let mut arguments = call.arguments.iter().cloned();
        let left: bool = argument(&mut arguments);
        let result = match self.schema().method_at(call.method).shape.text.as_str() {
            "not _" => BooleanOperation::Not.apply(left, false),
            "_ and _" => BooleanOperation::And.apply(left, argument(&mut arguments)),
            "_ or _" => BooleanOperation::Or.apply(left, argument(&mut arguments)),
            "_ xor _" => BooleanOperation::Xor.apply(left, argument(&mut arguments)),
            _ => unreachable!("the interpreter checked the method against the schema"),
        };
        Outcome::Value(result.into_value())
    }
}
