//! The prelude, `prelude.protocol.anyx`: the built-in types, declared in the schema
//! language with their docs. Its source is decoded once; ordinary declarations
//! are checked with each schema, and its names are visible in every scope.
//!
//! Intrinsic declarations name representations supplied by Rust; ordinary
//! declarations are checked alongside user types. For an intrinsic,
//! this module supplies its meaning, the [`Type`] its values have. The
//! codec reads and writes those; `time.rs` knows `Duration`'s units.

use std::sync::LazyLock;

use anyx::NamePart;

use crate::grammar as ast;
use crate::schema::Type;
use crate::value::Value;

/// The name the prelude's diagnostics are under.
pub const PATH: &str = "prelude.protocol.anyx";

/// The prelude as written.
pub const SOURCE: &str = include_str!("../prelude.protocol.anyx");

/// A built-in type, as the prelude declares it.
#[derive(Clone, Debug, PartialEq)]
pub struct Intrinsic {
    /// Its name, in Title Case: `Integer`.
    pub name: String,
    /// Its `#` lines, which say how a value of it is written.
    pub doc: String,
    /// The type the implementation supplies for it.
    pub r#type: Type,
}

/// What the implementation supplies for each name the prelude may declare.
/// The prelude must declare each of them, once.
fn implementation(name: &str) -> Option<Type> {
    Some(match name {
        "Text" => Type::Text,
        "Bytes" => Type::Bytes,
        "Integer" => Type::Integer,
        "Float" => Type::Float,
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

fn document() -> Result<&'static ast::Document, &'static [String]> {
    static DOCUMENT: LazyLock<Result<ast::Document, Vec<String>>> = LazyLock::new(|| {
        let parsed = anyx::parse(SOURCE);
        if !parsed.errors.is_empty() {
            return Err(parsed
                .errors
                .into_iter()
                .map(|error| error.message)
                .collect());
        }
        ast::decode_document(&parsed.syntax.nodes())
            .map_err(|error| vec![error.render(SOURCE, PATH)])
    });
    DOCUMENT.as_ref().map_err(Vec::as_slice)
}

/// Checks the prelude's intrinsic type declarations against the implementation.
fn load() -> Result<Vec<Intrinsic>, Vec<String>> {
    let document = document().map_err(<[String]>::to_vec)?;
    let mut problems = Vec::new();
    if document.header.is_some() {
        problems.push("found a `PROTOCOL` header; expected only `INTRINSIC` declarations".into());
    }
    let mut intrinsics: Vec<Intrinsic> = Vec::new();
    for definition in &document.definition {
        if !is_intrinsic(definition) {
            continue;
        }
        let declared = match definition {
            ast::Definition::IntrinsicType(ast::IntrinsicType { doc, name, .. }) => {
                Some((doc, name.name.words().collect::<Vec<_>>().join(" ")))
            }
            _ => None,
        };
        let Some((doc, name)) = declared else {
            problems.push(format!(
                "found `{}`; expected `INTRINSIC TYPE Name`, since the prelude declares only built-in types",
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
        intrinsics.push(Intrinsic { name, doc, r#type });
    }
    let all = [
        Type::Text,
        Type::Bytes,
        Type::Integer,
        Type::Float,
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

/// Ordinary prelude declarations use the same checker as user declarations.
pub(crate) fn declarations() -> ast::Document {
    static DECLARATIONS: LazyLock<ast::Document> = LazyLock::new(|| {
        let mut document = document().expect("the prelude parses").clone();
        functions();
        document.definition.retain(|definition| match definition {
            ast::Definition::Function(function) => function.intrinsic.is_some(),
            _ => !is_intrinsic(definition),
        });
        document
    });
    DECLARATIONS.clone()
}

fn is_intrinsic(definition: &ast::Definition) -> bool {
    match definition {
        ast::Definition::IntrinsicType(_) => true,
        ast::Definition::Record(d) => d.intrinsic.is_some(),
        ast::Definition::Errors(_) => false,
        _ => false,
    }
}

#[derive(Clone, Copy)]
pub(crate) enum Function {
    Compare(&'static [&'static str]),
    Arithmetic(Arithmetic),
    Length,
    Text(&'static str),
    Boolean(BooleanOperation),
    /// `_ has no repeats`: no two items of a list are equal.
    NoRepeats,
    /// `_ has at most one of _`: at most one item of a list is a variant of
    /// the given shapes.
    AtMostOneOf,
}

#[derive(Clone, Copy)]
pub(crate) enum Arithmetic {
    Add,
    Subtract,
    Multiply,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BooleanOperation {
    Not,
    And,
    Or,
    Xor,
}

impl BooleanOperation {
    pub(crate) fn word(self) -> &'static str {
        match self {
            Self::Not => "not",
            Self::And => "and",
            Self::Or => "or",
            Self::Xor => "xor",
        }
    }

    pub fn apply(self, left: bool, right: bool) -> bool {
        match self {
            Self::Not => !left,
            Self::And => left & right,
            Self::Or => left | right,
            Self::Xor => left ^ right,
        }
    }
}

impl Function {
    fn implementation(shape: &str) -> Option<Self> {
        use Arithmetic::{Add, Multiply, Subtract};
        use BooleanOperation::{And, Not, Or, Xor};
        Some(match shape {
            "_ + _" => Self::Arithmetic(Add),
            "_ - _" => Self::Arithmetic(Subtract),
            "_ * _" => Self::Arithmetic(Multiply),
            "_ < _" => Self::Compare(&["<"]),
            "_ <= _" => Self::Compare(&["<="]),
            "_ > _" => Self::Compare(&[">"]),
            "_ >= _" => Self::Compare(&[">="]),
            "_ = _" => Self::Compare(&["="]),
            "_ != _" => Self::Compare(&["!="]),
            "_ < _ < _" => Self::Compare(&["<", "<"]),
            "_ < _ <= _" => Self::Compare(&["<", "<="]),
            "_ <= _ < _" => Self::Compare(&["<=", "<"]),
            "_ <= _ <= _" => Self::Compare(&["<=", "<="]),
            "_ > _ > _" => Self::Compare(&[">", ">"]),
            "_ > _ >= _" => Self::Compare(&[">", ">="]),
            "_ >= _ > _" => Self::Compare(&[">=", ">"]),
            "_ >= _ >= _" => Self::Compare(&[">=", ">="]),
            "_ <= _ <= _ <= _" => Self::Compare(&["<=", "<=", "<="]),
            "_ length" => Self::Length,
            "_ starts with _" => Self::Text("starts with"),
            "_ ends with _" => Self::Text("ends with"),
            "_ contains _" => Self::Text("contains"),
            "not _" => Self::Boolean(Not),
            "_ and _" => Self::Boolean(And),
            "_ or _" => Self::Boolean(Or),
            "_ xor _" => Self::Boolean(Xor),
            "_ has no repeats" => Self::NoRepeats,
            "_ has at most one of _" => Self::AtMostOneOf,
            _ => return None,
        })
    }

    fn supports(self, parameters: &[&str], returns: &str) -> bool {
        match self {
            Self::Compare(operators) => {
                parameters.len() == operators.len() + 1
                    && ["Integer", "Float"]
                        .into_iter()
                        .any(|ty| parameters.iter().all(|p| *p == ty))
                    && returns == "Boolean"
            }
            Self::Arithmetic(_) => {
                ["Integer", "Float"].contains(&returns) && parameters == [returns, returns]
            }
            Self::Length => {
                matches!(parameters, ["Text" | "Bytes" | "LIST Type" | "MAP Type"])
                    && returns == "Integer"
            }
            Self::Text(_) => parameters == ["Text", "Text"] && returns == "Boolean",
            Self::Boolean(BooleanOperation::Not) => {
                parameters == ["Boolean"] && returns == "Boolean"
            }
            Self::Boolean(_) => parameters == ["Boolean", "Boolean"] && returns == "Boolean",
            Self::NoRepeats => parameters == ["LIST Type"] && returns == "Boolean",
            Self::AtMostOneOf => parameters == ["LIST Type", "LIST Type"] && returns == "Boolean",
        }
    }
}

fn signature_type(ty: &ast::Type) -> &'static str {
    match ty {
        ast::Type::Named(reference) if reference.arguments.is_empty() => {
            match reference.name.text.as_str() {
                "Text" => "Text",
                "Bytes" => "Bytes",
                "Integer" => "Integer",
                "Float" => "Float",
                "Boolean" => "Boolean",
                "Type" => "Type",
                _ => "",
            }
        }
        ast::Type::List(list) if signature_type(&list.item) == "Type" => "LIST Type",
        ast::Type::Map(map) if signature_type(&map.value) == "Type" => "MAP Type",
        _ => "",
    }
}

struct DeclaredFunction {
    implementation: Function,
    overloads: Vec<Vec<&'static str>>,
}

fn functions() -> &'static std::collections::HashMap<String, DeclaredFunction> {
    static FUNCTIONS: LazyLock<std::collections::HashMap<String, DeclaredFunction>> =
        LazyLock::new(|| {
            let mut functions = std::collections::HashMap::new();
            for definition in &document().expect("the prelude parses").definition {
                if let ast::Definition::Function(function) = definition
                    && function.intrinsic.is_none()
                {
                    let shape = &function.shape.name.text;
                    let implementation = Function::implementation(shape).unwrap_or_else(|| {
                        panic!("no implementation of prelude FUNCTION `{shape}`")
                    });
                    let parameters = function
                        .shape
                        .arguments
                        .iter()
                        .flat_map(|part| match part {
                            ast::ShapePart::Parameter(parameter) => vec![parameter],
                            ast::ShapePart::Optional(optional) => {
                                optional.value.part.arguments.iter().collect()
                            }
                        })
                        .map(|p| signature_type(&p.r#type))
                        .collect::<Vec<_>>();
                    let returns = signature_type(&function.returns.r#type);
                    assert!(
                        implementation.supports(&parameters, returns),
                        "unsupported signature for prelude FUNCTION `{shape}`"
                    );
                    let declared =
                        functions
                            .entry(shape.clone())
                            .or_insert_with(|| DeclaredFunction {
                                implementation,
                                overloads: Vec::new(),
                            });
                    assert!(
                        !declared.overloads.contains(&parameters),
                        "duplicate signature for prelude FUNCTION `{shape}`"
                    );
                    declared.overloads.push(parameters);
                }
            }
            functions
        });
    &FUNCTIONS
}

/// The value a call of the `INTRINSIC FUNCTION` of the shape `shape`
/// makes from `arguments`, those of its full form. `Err` says what is
/// wrong with them.
pub(crate) fn evaluate_intrinsic(shape: &str, arguments: &[Value]) -> Result<Value, String> {
    match shape {
        "_ days _ hours _ minutes _ seconds _ milliseconds" => {
            let counts: Vec<i64> = arguments
                .iter()
                .map(|argument| match argument {
                    Value::Integer(count) => *count,
                    _ => unreachable!("a checked call of `Duration` has Integer counts"),
                })
                .collect();
            crate::time::duration_of(&counts).map(Value::Duration)
        }
        _ => unreachable!("no implementation of the prelude's INTRINSIC FUNCTION `{shape}`"),
    }
}

pub(crate) fn function(shape: &str) -> Option<Function> {
    functions()
        .get(shape)
        .map(|function| function.implementation)
}

pub(crate) fn function_takes(shape: &str, parameters: &[&str]) -> bool {
    functions().get(shape).is_some_and(|function| {
        function
            .overloads
            .iter()
            .any(|overload| overload == parameters)
    })
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
            ["Text", "Bytes", "Integer", "Float", "Instant", "Duration"]
        );
    }

    #[test]
    fn an_old_all_caps_name_finds_its_new_one() {
        assert_eq!(renamed("INTEGER").unwrap().name, "Integer");
        assert_eq!(renamed("LIST"), None);
    }
}
