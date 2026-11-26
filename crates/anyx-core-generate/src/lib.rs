//! Generates a Rust server and a TypeScript client from a schema written in
//! the anyx core.
//!
//! - A `DATA` becomes a type in both, sent as JSON with its constructor's
//!   name under `kind`.
//! - A `FUNCTION` without a body is served: a method of the Rust `Server`
//!   trait.
//! - A `FUNCTION` with a body is compiled to Rust as written.
//! - A bodied function with a parameter of an indexed family is an entry
//!   point: the client calls it, one method per constructor, and the server
//!   checks each argument against the type that constructor fixes before
//!   running it.
//!
//! See [`model`] for how types and functions are read.

pub mod model;
mod names;
mod rust;
mod typescript;

use anyx::{Span, line_column};
use anyx_core::{
    kernel::{self, TypeChecker},
    lowering, parser,
};
use std::fmt;

pub use model::Schema;
pub use names::{camel_case, pascal_case, snake_case};
pub use rust::rust;
pub use typescript::typescript;

/// Why a schema has no generated code.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    pub message: String,
}

impl Error {
    pub(crate) fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for Error {}

/// Parse, check and read a schema. `file` names it in errors.
pub fn load(source: &str, file: &str) -> Result<Schema, Error> {
    let parsed = parser::parse_document(source);
    if let Some(error) = parsed.syntax_errors.first() {
        return Err(Error::new(error.render(source, file)));
    }
    let syntax = parsed
        .document
        .map_err(|error| Error::new(error.render(source, file)))?;
    let lowered = lowering::lower_document(&syntax)
        .map_err(|error| Error::new(render(source, file, error.span, &error)))?;
    let document = kernel::Document::try_from(&lowered)
        .map_err(|error| Error::new(render(source, file, syntax.span, error)))?;
    let checker = TypeChecker::default()
        .check_document(&document)
        .map_err(|error| Error::new(render(source, file, error.span(&syntax), &error)))?;
    model::build(checker, &document, &lowered)
}

fn render(source: &str, file: &str, span: Span, error: impl fmt::Display) -> String {
    let position = line_column(source, span.start);
    format!("{file}:{position}: {error}")
}
