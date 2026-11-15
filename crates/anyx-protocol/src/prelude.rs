//! The prelude, `prelude.anyx`: the built-in types, declared in the schema
//! language with their docs. It is read and checked once, before the first
//! schema, and its names are visible in every scope of every schema.
//!
//! Each declaration is `INTRINSIC`: the prelude writes its name and shape,
//! and this module supplies its meaning, the [`Type`] its values have. The
//! codec reads and writes those; for `Duration`, the prelude's variants are
//! the units, and `time.rs` knows how long each is.

use std::sync::LazyLock;

use anyx::NamePart;

use crate::grammar as ast;
use crate::schema::Type;

/// The name the prelude's diagnostics are under.
pub const PATH: &str = "prelude.anyx";

/// The prelude as written.
pub const SOURCE: &str = include_str!("../prelude.anyx");

/// A built-in type, as the prelude declares it.
#[derive(Clone, Debug, PartialEq)]
pub struct Intrinsic {
    /// Its name, in Title Case: `Integer`.
    pub name: String,
    /// Its `#` lines, which say how a value of it is written.
    pub doc: String,
    /// The type the implementation supplies for it.
    pub r#type: Type,
    /// The variants of an `INTRINSIC CHOICE`, in order: `Duration`'s
    /// units, `day` to `millisecond`. Empty for an `INTRINSIC TYPE`.
    pub variants: Vec<String>,
}

/// What the implementation supplies for each name the prelude may declare.
/// The prelude must declare each of them, once.
fn implementation(name: &str) -> Option<Type> {
    Some(match name {
        "Text" => Type::Text,
        "Bytes" => Type::Bytes,
        "Integer" => Type::Integer,
        "Float" => Type::Float,
        "Boolean" => Type::Boolean,
        "Instant" => Type::Instant,
        "Duration" => Type::Duration,
        _ => return None,
    })
}

/// The built-in types, in the order the prelude declares them.
pub fn intrinsics() -> &'static [Intrinsic] {
    static INTRINSICS: LazyLock<Vec<Intrinsic>> = LazyLock::new(|| {
        load().unwrap_or_else(|problems| {
            panic!(
                "the prelude of anyx-protocol is broken:\n{}",
                problems.join("\n")
            )
        })
    });
    &INTRINSICS
}

/// The built-in type named `name`, such as `Integer`.
pub fn find(name: &str) -> Option<&'static Intrinsic> {
    intrinsics().iter().find(|intrinsic| intrinsic.name == name)
}

/// The prelude's declaration of `type`, when it is a built-in type.
pub fn of(r#type: &Type) -> Option<&'static Intrinsic> {
    intrinsics()
        .iter()
        .find(|intrinsic| &intrinsic.r#type == r#type)
}

/// The built-in type a schema's `reference` names: a name of the prelude,
/// alone, with no slots and no path.
pub(crate) fn reference(reference: &ast::Reference) -> Option<Type> {
    let plain = reference.arguments.is_empty()
        && reference
            .name
            .parts
            .iter()
            .all(|part| matches!(part, NamePart::Word { text, .. } if text != "/"));
    plain
        .then(|| find(&reference.name.text))
        .flatten()
        .map(|intrinsic| intrinsic.r#type.clone())
}

/// The built-in type whose name is `word` in all caps, as the built-in
/// types were written before the prelude: `INTEGER` for `Integer`.
pub(crate) fn renamed(word: &str) -> Option<&'static Intrinsic> {
    intrinsics()
        .iter()
        .find(|intrinsic| intrinsic.name.to_uppercase() == word)
}

/// Reads and checks the prelude: every declaration is `INTRINSIC`, has a
/// doc, and is one the implementation supplies, and every one it supplies
/// is declared. `Err` lists what is wrong.
fn load() -> Result<Vec<Intrinsic>, Vec<String>> {
    let parsed = anyx::parse(SOURCE);
    if !parsed.errors.is_empty() {
        return Err(parsed
            .errors
            .iter()
            .map(|error| error.message.clone())
            .collect());
    }
    let document = ast::decode_document(&parsed.syntax.nodes())
        .map_err(|error| vec![error.render(SOURCE, PATH)])?;
    let mut problems = Vec::new();
    if document.header.is_some() {
        problems.push("found a `PROTOCOL` header; expected only `INTRINSIC` declarations".into());
    }
    let mut intrinsics: Vec<Intrinsic> = Vec::new();
    for definition in &document.definition {
        let declared = match definition {
            ast::Definition::NamedType(ast::NamedType {
                doc,
                intrinsic: Some(_),
                body: ast::NamedTypeBody::Bare { name, .. },
                ..
            }) => Some((doc, name.value.clone(), Vec::new())),
            ast::Definition::Choice(ast::Choice {
                doc,
                intrinsic: Some(_),
                body,
                ..
            }) => match body.arguments.as_slice() {
                [ast::ChoicePart::Variants(variants)]
                    if variants
                        .value
                        .iter()
                        .all(|variant| variant.arguments.is_empty()) =>
                {
                    let variants = variants
                        .value
                        .iter()
                        .map(|variant| variant.name.text.clone())
                        .collect();
                    Some((
                        doc,
                        body.name.words().collect::<Vec<_>>().join(" "),
                        variants,
                    ))
                }
                _ => None,
            },
            _ => None,
        };
        let Some((doc, name, variants)) = declared else {
            problems.push(format!(
                "found `{}`; expected `INTRINSIC TYPE Name` or `INTRINSIC CHOICE Name {{ … }}` with variants of words alone, since the prelude declares only built-in types",
                &SOURCE[definition.span().start..definition.span().end]
            ));
            continue;
        };
        let Some(r#type) = implementation(&name) else {
            problems.push(format!(
                "found the built-in type `{name}`, which the implementation doesn't supply"
            ));
            continue;
        };
        let Some(doc) = doc.as_ref().map(|doc| doc.value.value.clone()) else {
            problems.push(format!(
                "found `{name}` without `#` lines; expected them, since the guide shows them"
            ));
            continue;
        };
        if intrinsics.iter().any(|known| known.name == name) {
            problems.push(format!("found `{name}` twice; expected it once"));
            continue;
        }
        if r#type == Type::Duration
            && let Err(problem) = crate::time::check_units(&variants)
        {
            problems.push(problem);
        }
        intrinsics.push(Intrinsic {
            name,
            doc,
            r#type,
            variants,
        });
    }
    let all = [
        Type::Text,
        Type::Bytes,
        Type::Integer,
        Type::Float,
        Type::Boolean,
        Type::Instant,
        Type::Duration,
    ];
    for r#type in all {
        if !intrinsics
            .iter()
            .any(|intrinsic| intrinsic.r#type == r#type)
        {
            problems.push(format!(
                "found no declaration of the built-in type {type:?}; expected one"
            ));
        }
    }
    match problems.is_empty() {
        true => Ok(intrinsics),
        false => Err(problems),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_prelude_declares_every_built_in_type() {
        let names: Vec<&str> = intrinsics()
            .iter()
            .map(|intrinsic| intrinsic.name.as_str())
            .collect();
        assert_eq!(
            names,
            [
                "Text", "Bytes", "Integer", "Float", "Boolean", "Instant", "Duration"
            ]
        );
        assert_eq!(
            find("Duration").unwrap().variants,
            ["day", "hour", "minute", "second", "millisecond"]
        );
    }

    #[test]
    fn an_old_all_caps_name_finds_its_new_one() {
        assert_eq!(renamed("INTEGER").unwrap().name, "Integer");
        assert_eq!(renamed("LIST"), None);
    }
}
