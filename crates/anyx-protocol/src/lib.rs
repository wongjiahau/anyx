//! The anyx protocol: an app declares its methods in a schema written in
//! anyx, and requests and responses are anyx documents checked against it.
//! See `PROTOCOL.md` for the schema language and the wire format.

mod annotation;
mod bytes;
mod check;
pub mod client;
pub mod codec;
mod error_terms;
// Generated from `protocol.anyx`; its definitions are as big as they come.
#[allow(clippy::large_enum_variant)]
mod grammar;
mod guide;
#[cfg(feature = "json")]
pub mod json;
pub mod prelude;
mod response;
pub mod schema;
pub mod server;
mod time;
pub mod value;

pub use codec::{Call, EncodeError, Outcome, Problem, Response, decode_request, encode_request};
pub use guide::guide;
pub use response::{decode_response, encode_response};

/// Formats a schema canonically: a long `METHOD` puts `RETURNS` and `ERRORS`
/// on lines of their own.
pub fn format_schema(source: &str) -> Result<String, Vec<anyx::Error>> {
    grammar::format(source)
}
pub use schema::{Diagnostic, Schema, SchemaFile};
pub use value::{FieldsBuilder, FieldsReader, FromValue, IntoValue, Value};

mod request;
mod request_grammar;
