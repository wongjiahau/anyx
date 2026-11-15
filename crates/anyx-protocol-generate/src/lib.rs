//! Generates the code of an [`anyx_protocol`] schema:
//!
//! - [`rust`]: the schema's types, a `Server` trait with `handle`, and a
//!   `Client`.
//! - [`wasm`]: `#[wasm_bindgen]` functions that encode a call and decode its
//!   response as JSON, for the TypeScript client.
//! - [`typescript`]: the schema's types and a `Client` that calls into the
//!   wasm functions, so encoding and decoding stay in Rust.

mod names;
pub(crate) mod rust;
mod typescript;
mod wasm;

use std::collections::BTreeMap;

use anyx_protocol::Schema;
use anyx_protocol::schema::{Effect, Method, Type, TypeKind, scoped};

pub use names::{camel_case, pascal_case, snake_case};
pub use rust::rust;
pub use typescript::typescript;
pub use wasm::wasm;

/// Formats generated Rust with `rustfmt`.
pub use anyx_generator::format as format_rust;

/// Why a schema has no code: two of its names become the same name in the
/// generated code.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    pub message: String,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for Error {}

/// The `EXAMPLES` of `method` for its doc comment: a heading, then a code
/// block of each example as written, after its `#` doc. `None` without any.
pub fn examples_doc(method: &Method) -> Option<String> {
    if method.examples.is_empty() {
        return None;
    }
    let examples: String = method
        .examples
        .iter()
        .map(|example| {
            let doc: String = example
                .doc
                .iter()
                .flat_map(|doc| doc.lines())
                .map(|line| match line.is_empty() {
                    true => "#\n".to_string(),
                    false => format!("# {line}\n"),
                })
                .collect();
            format!("{doc}{}\n", example.text)
        })
        .collect();
    Some(format!("Examples:\n\n```anyx\n{examples}```"))
}

/// What `method`'s markers claim, for its doc comment, in a sentence or two.
pub fn effects_doc(method: &Method) -> String {
    let effect = match method.effect {
        Effect::ReadOnly => "Read only: it changes nothing, so it is safe to call freely.",
        Effect::Idempotent => {
            "Idempotent: calling it again with the same arguments changes nothing more, so it is safe to retry after a lost response."
        }
        Effect::Unmarked => {
            "It may change anything, and calling it again may do it again, so don't retry it blindly."
        }
    };
    match &method.undone_by {
        Some(undo) => format!("{effect} Undone by `{}`.", undo.text),
        None => effect.to_string(),
    }
}

/// The words that name `method` in generated code: its shape's, or for an
/// overload, its shape's then `with` and its parameters' types, joined by
/// `and`, so that each overload has a name of its own:
/// `tag (names: LIST: TEXT)` is `tag with list of text`, and
/// `open (id: file id)` is `open with file id`. A method without overloads
/// keeps its shape's words.
pub fn method_words(schema: &Schema, method: &Method) -> String {
    if !schema.is_overloaded(method) {
        return method.shape.text.clone();
    }
    let types: Vec<String> = method
        .shape
        .parameters
        .iter()
        .map(|parameter| type_words(schema, &parameter.r#type))
        .collect();
    format!("{} with {}", method.shape.text, types.join(" and "))
}

/// A type in lowercase words, for a name: `LIST: TEXT` is `list of text`,
/// `common/file id` is `common file id`, and `page of (INTEGER)` is
/// `page of integer`.
fn type_words(schema: &Schema, r#type: &Type) -> String {
    let named = |index: usize, arguments: &[Type]| {
        let definition = &schema.types[index];
        let mut arguments = arguments.iter();
        definition
            .scope
            .iter()
            .cloned()
            .chain(definition.name.split(' ').map(|word| {
                match word {
                    "_" => arguments
                        .next()
                        .map_or_else(String::new, |argument| type_words(schema, argument)),
                    word => word.to_string(),
                }
            }))
            .collect::<Vec<_>>()
            .join(" ")
    };
    match r#type {
        Type::List(item) => format!("list of {}", type_words(schema, item)),
        Type::Map(value) => format!("map of {}", type_words(schema, value)),
        Type::Named(index) => named(*index, &[]),
        Type::Generic(index, arguments) => named(*index, arguments),
        Type::Parameter(name) => name.clone(),
        built_in => schema.type_name(built_in).to_lowercase(),
    }
}

/// The enum of a method's errors, in the module of its scope:
/// `write _ at revision _ content _` has `WriteAtRevisionContentError`.
pub fn error_enum_name(schema: &Schema, method: &Method) -> String {
    format!("{}Error", pascal_case(&method_words(schema, method)))
}

/// A name in TypeScript, where every scope shares one namespace, so the
/// scope's words come first: `diagnostic` in `common` is
/// `CommonDiagnostic`.
pub fn scoped_pascal_case(scope: &[String], name: &str) -> String {
    scope
        .iter()
        .map(|segment| pascal_case(segment))
        .collect::<String>()
        + &pascal_case(name)
}

/// The TypeScript union of a method's errors: `FilesReadError`.
pub fn typescript_error_name(schema: &Schema, method: &Method) -> String {
    format!(
        "{}Error",
        scoped_pascal_case(&method.scope, &method_words(schema, method))
    )
}

/// The scopes directly in `scope`.
pub fn children(schema: &Schema, scope: &[String]) -> Vec<Vec<String>> {
    schema
        .scopes()
        .into_iter()
        .filter(|child| child.len() == scope.len() + 1 && child.starts_with(scope))
        .collect()
}

/// Whether `scope`, or a scope in it, has a method, so that the client
/// reaches it.
pub fn has_methods(schema: &Schema, scope: &[String]) -> bool {
    schema
        .methods
        .iter()
        .any(|method| method.scope.starts_with(scope))
}

/// The methods declared in `scope` itself.
pub fn methods_in<'s>(schema: &'s Schema, scope: &[String]) -> Vec<(usize, &'s Method)> {
    schema
        .methods
        .iter()
        .enumerate()
        .filter(|(_, method)| method.scope == scope)
        .collect()
}

/// Names the generated code declares or uses, which a schema's types would
/// shadow.
const RESERVED_TYPE_NAMES: [&str; 18] = [
    "Client",
    "Server",
    "Value",
    "String",
    "Vec",
    "Option",
    "Result",
    "Outcome",
    "FieldsBuilder",
    "FieldsReader",
    "FromValue",
    "IntoValue",
    "CallError",
    "Transport",
    // TypeScript's own types for `INSTANT` and `MAP`, and the client's.
    "Date",
    "Record",
    "Reviver",
    // The TypeScript client's description of a method's markers.
    "Effects",
];

/// The name of a type parameter in the generated code: the parameter in
/// PascalCase, with `Parameter` after it while that is a name `taken`, such
/// as a type of the schema, so that the parameter doesn't shadow it: `item`
/// is `Item`, and `value` is `ValueParameter`, since the Rust code uses its
/// `Value`.
pub fn type_parameter_name(name: &str, taken: impl Fn(&str) -> bool) -> String {
    let mut parameter = pascal_case(name);
    while taken(&parameter) {
        parameter.push_str("Parameter");
    }
    parameter
}

/// Whether `name` is a type a type parameter in the Rust module of `scope`
/// would shadow: the scope's types, the errors of its methods, and the
/// names the generated code uses.
pub fn rust_type_taken(schema: &Schema, scope: &[String], name: &str) -> bool {
    RESERVED_TYPE_NAMES.contains(&name)
        || name == "Self"
        || schema
            .types
            .iter()
            .any(|definition| definition.scope == scope && pascal_case(&definition.name) == name)
        || methods_in(schema, scope)
            .into_iter()
            .any(|(_, method)| !method.errors.is_empty() && error_enum_name(schema, method) == name)
}

/// Whether `name` is a type a type parameter in the TypeScript code would
/// shadow: any type of the schema, the errors of a method, and the names
/// the client declares or uses. `Instant` is taken too, since a parameter's
/// reviver is `revive` and its name, and `reviveInstant` revives instants.
pub fn typescript_type_taken(schema: &Schema, name: &str) -> bool {
    RESERVED_TYPE_NAMES.contains(&name)
        || ["Promise", "Codec", "QueuedCall", "Instant"].contains(&name)
        || schema
            .types
            .iter()
            .any(|definition| scoped_pascal_case(&definition.scope, &definition.name) == name)
        || schema.methods.iter().any(|method| {
            !method.errors.is_empty() && typescript_error_name(schema, method) == name
        })
}

/// Names the Rust and TypeScript clients declare next to the methods.
const RESERVED_METHOD_NAMES: [&str; 5] = ["new", "call", "codec", "send", "constructor"];

fn scope_name(scope: &[String]) -> String {
    match scope.is_empty() {
        true => "the root".to_string(),
        false => format!("the scope `{}`", scope.join("/")),
    }
}

/// Checks that every name the code declares in one place is distinct there:
/// in Rust, a module per scope; in TypeScript, one namespace for the types
/// and an object per scope for the methods.
fn check_names(schema: &Schema) -> Result<(), Error> {
    let reserved_types = || {
        RESERVED_TYPE_NAMES
            .iter()
            .map(|name| (name.to_string(), format!("the generated `{name}`")))
    };
    let reserved_methods = || {
        RESERVED_METHOD_NAMES
            .iter()
            .map(|name| (name.to_string(), format!("the client's `{name}`")))
    };
    for scope in schema.scopes() {
        let types = schema
            .types
            .iter()
            .filter(|definition| definition.scope == scope)
            .map(|definition| {
                (
                    pascal_case(&definition.name),
                    format!("the type `{}`", definition.shape()),
                )
            });
        let error_enums = methods_in(schema, &scope)
            .into_iter()
            .filter(|(_, method)| !method.errors.is_empty())
            .map(|(_, method)| {
                (
                    error_enum_name(schema, method),
                    format!("the errors of `{}`", schema.call_name(method)),
                )
            });
        distinct("type", reserved_types().chain(types).chain(error_enums))?;
        let methods = methods_in(schema, &scope).into_iter().map(|(_, method)| {
            (
                snake_case(&method_words(schema, method)),
                format!("`{}`", schema.call_name(method)),
            )
        });
        let scopes = children(schema, &scope).into_iter().map(|child| {
            (
                snake_case(child.last().expect("a child scope has a name")),
                format!("the scope `{}` in {}", child.join("/"), scope_name(&scope)),
            )
        });
        let introspection = scope.is_empty().then(|| {
            (
                "schema".to_string(),
                "the client's `schema`, which calls `SCHEMA`".to_string(),
            )
        });
        distinct(
            "method",
            reserved_methods()
                .chain(introspection)
                .chain(methods)
                .chain(scopes),
        )?;
        if let Some(segment) = scope
            .iter()
            .find(|segment| ["self", "super", "crate"].contains(&segment.as_str()))
        {
            return Err(Error {
                message: format!(
                    "found the scope `{}`; expected a scope that isn't named `{segment}`, which Rust reserves for module paths",
                    scope.join("/")
                ),
            });
        }
    }
    let typescript_types = schema
        .types
        .iter()
        .map(|definition| {
            (
                scoped_pascal_case(&definition.scope, &definition.name),
                format!(
                    "the type `{}`",
                    scoped(&definition.scope, &definition.shape())
                ),
            )
        })
        .chain(
            schema
                .methods
                .iter()
                .filter(|method| !method.errors.is_empty())
                .map(|method| {
                    (
                        typescript_error_name(schema, method),
                        format!("the errors of `{}`", schema.call_name(method)),
                    )
                }),
        );
    distinct("TypeScript type", reserved_types().chain(typescript_types))?;
    for definition in &schema.types {
        let parameter =
            |name: &String| format!("the type parameter `{name}` of `{}`", definition.shape());
        distinct(
            "type parameter",
            definition.parameters.iter().map(|name| {
                (
                    type_parameter_name(name, |taken| {
                        rust_type_taken(schema, &definition.scope, taken)
                    }),
                    parameter(name),
                )
            }),
        )?;
        distinct(
            "TypeScript type parameter",
            definition.parameters.iter().map(|name| {
                (
                    type_parameter_name(name, |taken| typescript_type_taken(schema, taken)),
                    parameter(name),
                )
            }),
        )?;
        match &definition.kind {
            TypeKind::Record(fields) => distinct(
                "field",
                fields
                    .iter()
                    .map(|field| (snake_case(&field.name), format!("`{}`", field.name))),
            )?,
            TypeKind::Choice(variants) => distinct(
                "variant",
                variants.iter().map(|variant| {
                    (
                        pascal_case(&variant.shape.text),
                        format!("`{}`", variant.shape.text),
                    )
                }),
            )?,
            TypeKind::Distinct(_) => {}
        }
    }
    for method in &schema.methods {
        distinct(
            "error",
            schema.errors_of(method).into_iter().map(|error| {
                (
                    pascal_case(&error.shape.text),
                    format!("`{}`", error.shape.text),
                )
            }),
        )?;
    }
    for shape in schema
        .methods
        .iter()
        .map(|method| &method.shape)
        .chain(schema.errors.iter().map(|error| &error.error.shape))
        .chain(
            schema
                .types
                .iter()
                .flat_map(|definition| match &definition.kind {
                    TypeKind::Choice(variants) => {
                        variants.iter().map(|variant| &variant.shape).collect()
                    }
                    TypeKind::Record(_) | TypeKind::Distinct(_) => Vec::new(),
                }),
        )
    {
        distinct(
            "parameter",
            shape.parameters.iter().map(|parameter| {
                (
                    snake_case(&parameter.name),
                    format!("`{}` in `{}`", parameter.name, shape.text),
                )
            }),
        )?;
    }
    Ok(())
}

/// Fails when two of `names` are the same, naming what each came from.
fn distinct(kind: &str, names: impl Iterator<Item = (String, String)>) -> Result<(), Error> {
    let mut seen: BTreeMap<String, String> = BTreeMap::new();
    for (name, from) in names {
        if let Some(earlier) = seen.get(&name) {
            return Err(Error {
                message: format!(
                    "found {from}, which becomes the {kind} `{name}`, like {earlier}; expected names that stay distinct in the generated code, so rename one"
                ),
            });
        }
        seen.insert(name, from);
    }
    Ok(())
}
