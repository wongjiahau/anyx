//! A protocol has one flat namespace, split across explicitly imported files.

use anyx::{NamePart, Node, NodeKind, Span, line_column};

use crate::check::Checker;
use crate::codec::Outcome;
use crate::grammar as ast;
use crate::value::Value;

/// A source file, useful for embedding a protocol in generated code.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SchemaFile<'a> {
    pub path: &'a str,
    pub source: &'a str,
}

/// A schema that passed every check.
#[derive(Clone, Debug, PartialEq)]
pub struct Schema {
    /// The files as written, which the guide shows as they are: the root
    /// first, then each file in import traversal order.
    pub files: Vec<SourceFile>,
    pub doc: Option<String>,
    pub name: String,
    pub version: u32,
    pub types: Vec<TypeDefinition>,
    pub errors: Vec<ErrorDefinition>,
    pub methods: Vec<Method>,
}

/// A file of a checked schema.
#[derive(Clone, Debug, PartialEq)]
pub struct SourceFile {
    pub path: String,
    pub source: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TypeDefinition {
    /// `PUBLIC`: direct importers may use it.
    pub public: bool,
    pub doc: Option<String>,
    /// Its shape: lowercase words, such as `file summary`, with `_` for
    /// each slot of a generic record or choice, such as `page of _` for
    /// `RECORD Page of (item: TYPE)`. A type is known by its words and
    /// where its slots sit, so `page` and `page of _` are two types.
    pub name: String,
    /// The type parameters of a generic record or choice, one per slot of
    /// its shape, in order: `(value: TYPE) or (problem: TYPE)` has `value`
    /// and `problem`. Empty for any other type.
    pub parameters: Vec<String>,
    pub kind: TypeKind,
}

impl TypeDefinition {
    /// Its shape with each parameter in its slot, as a use of it is
    /// written: `page of (item)`, or `file summary` for a type that isn't
    /// generic.
    pub fn shape(&self) -> String {
        fill_slots(&self.name, &self.parameters)
    }
}

/// `text`, a shape with `_` for each slot, with each slot filled by the next
/// of `arguments` in parentheses: `page of _` with `Text` is
/// `page of (Text)`. A slot left over stays `_`.
pub fn fill_slots(text: &str, arguments: &[String]) -> String {
    let mut arguments = arguments.iter();
    text.split(' ')
        .map(|word| match (word, word == "_") {
            (_, true) => arguments
                .next()
                .map_or_else(|| "_".to_string(), |argument| format!("({argument})")),
            (word, false) => word.to_string(),
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[derive(Clone, Debug, PartialEq)]
pub enum TypeKind {
    Record(Vec<Field>),
    /// Variants, each written in its own shape.
    Choice(Vec<Shaped>),
    /// A named type, `TYPE File Id: Text`: the type underneath, under a name
    /// of its own. Generated code keeps it distinct from that type and from
    /// other named types over it; the wire and JSON write it exactly like
    /// the type underneath.
    Distinct(Type),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Field {
    pub doc: Option<String>,
    pub name: String,
    pub r#type: Type,
    /// `OPTIONAL`: the field may be left out.
    pub optional: bool,
    /// What an `OPTIONAL` field with a `DEFAULT` holds when it is left
    /// out. The codec fills it in, so a record read from the wire always
    /// has the field.
    pub default: Option<crate::value::Value>,
}

/// A call's shape: words with a typed slot between or around them, such as
/// `conflict at revision (revision: Integer)`.
#[derive(Clone, Debug, PartialEq)]
pub struct Shape {
    /// The words with `_` for each slot: `conflict at revision _`.
    pub text: String,
    pub parameters: Vec<Parameter>,
}

impl Shape {
    /// The words alone, which name it: `conflict at revision`.
    pub fn words(&self) -> String {
        self.text
            .split(' ')
            .filter(|word| *word != "_")
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// A choice's variant, or an error: a shape with a doc.
#[derive(Clone, Debug, PartialEq)]
pub struct Shaped {
    pub doc: Option<String>,
    pub shape: Shape,
}

/// An error, defined in the brackets of a method's or a group's `ERRORS`.
/// On the wire it is written in its shape alone, without a file path.
#[derive(Clone, Debug, PartialEq)]
pub struct ErrorDefinition {
    pub public: bool,
    pub error: Shaped,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Method {
    pub doc: Option<String>,
    pub shape: Shape,
    /// What a call returns when it succeeds, named, or `None` for a
    /// method that `RETURNS NOTHING`.
    pub returns: Option<Parameter>,
    /// Indices into [`Schema::errors`], with aliases expanded, in the order
    /// written.
    pub errors: Vec<usize>,
    /// Its `EXAMPLES`, in the order written, each checked against it.
    pub examples: Vec<Example>,
    /// What a call may do to the world: `READ ONLY`, `IDEMPOTENT`, or
    /// neither.
    pub effect: Effect,
    /// The call that undoes it, from `UNDONE BY`.
    pub undone_by: Option<Undo>,
}

/// What a call of a method may do, as its markers claim. Each marker is a
/// claim of safety, so a method without one is the worst case.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Effect {
    /// No marker: a call may change anything, and calling it again may do
    /// it again, so it is never retried on its own.
    #[default]
    Unmarked,
    /// `IDEMPOTENT`: calling it again with the same arguments leaves the
    /// same state as calling it once, even when the second response
    /// differs. Safe to retry after a lost response.
    Idempotent,
    /// `READ ONLY`: a call asks for no change, so it is also idempotent.
    /// Safe to call freely.
    ReadOnly,
}

impl Effect {
    pub fn read_only(self) -> bool {
        self == Effect::ReadOnly
    }

    /// Whether calling it again is safe: `IDEMPOTENT`, or `READ ONLY`, which
    /// implies it.
    pub fn idempotent(self) -> bool {
        self != Effect::Unmarked
    }
}

/// `UNDONE BY move (new path: Text) to (path: Text)`: the call of another
/// method of the same protocol that compensates a call of this one, with
/// this method's parameters as its arguments. It also claims that the call
/// disclosed nothing outside the account that undoing would not take back.
#[derive(Clone, Debug, PartialEq)]
pub struct Undo {
    /// The undoing method, an index into [`Schema::methods`].
    pub method: usize,
    /// For each parameter of the undoing method, the index of the parameter
    /// of this method that fills it.
    pub arguments: Vec<usize>,
    /// The call as written after `UNDONE BY`, such as
    /// `move (new path: Text) to (path: Text)`.
    pub text: String,
}

/// An example of a method: a call of it and its outcome side by side, as
/// in `write "a" at revision 3 content "x" RETURNS 4`.
#[derive(Clone, Debug, PartialEq)]
pub struct Example {
    pub doc: Option<String>,
    /// The call and its outcome as written in the schema, without the doc.
    pub text: String,
    /// The call alone as written, which a request may send as it is.
    pub call: String,
    /// The call's arguments, one per parameter of the method.
    pub arguments: Vec<Value>,
    /// What the call returns, [`Outcome::Value`], or the error it returns,
    /// [`Outcome::Error`].
    pub outcome: Outcome,
}

impl Method {
    /// The method shape, without any file path.
    pub fn full_name(&self) -> String {
        self.shape.text.clone()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Parameter {
    pub name: String,
    pub r#type: Type,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Type {
    Text,
    /// Raw bytes, such as a file. On the wire, a base64 string.
    Bytes,
    Integer,
    /// A finite 64-bit IEEE 754 float: `f64` in Rust, `number` in
    /// TypeScript. Written with a point or an exponent, `1.5`, `3.0`, `1e3`.
    Float,
    Boolean,
    /// A point in time, to the millisecond. On the wire, RFC 3339 text in
    /// UTC: `"2026-11-14T06:00:08.000Z"`.
    Instant,
    /// A length of time, to the millisecond. On the wire, whole numbers
    /// with their units: `1 hour 30 minutes`.
    Duration,
    /// A type narrowed by a checked predicate: `(Integer) WHERE [1 <= _]`.
    Refined(Box<Type>, crate::refinement::Predicate),
    List(Box<Type>),
    /// Text keys, each to a value of the type. On the wire, braces of keys
    /// and values: `{ "hymn" 3, "ode" 1 }`.
    Map(Box<Type>),
    /// The index of a type in [`Schema::types`], one that isn't generic.
    Named(usize),
    /// A generic record or choice with a type in each slot, `page of
    /// (Text)`: the index of the type in [`Schema::types`], and a type for
    /// each of its parameters, in the order of its slots.
    Generic(usize, Vec<Type>),
    /// A type parameter, by its name, within the generic record or choice
    /// that declares it. A use of that type puts its argument in its place,
    /// so a value is never of this type.
    Parameter(String),
}

impl Type {
    /// Whether an argument's type matches this parameter before checking its
    /// value against inline refinements. Named types retain their identity.
    /// This is only a type check; the parameter's predicates must still run.
    pub fn accepts_argument_type(&self, argument: &Type) -> bool {
        if self == argument {
            return true;
        }
        match (self, argument) {
            (Type::Refined(base, _), argument) => base.accepts_argument_type(argument),
            (Type::List(expected), Type::List(actual))
            | (Type::Map(expected), Type::Map(actual)) => expected.accepts_argument_type(actual),
            _ => false,
        }
    }

    /// `self` with each of `parameters` replaced by the argument at its
    /// place in `arguments`.
    pub fn substitute(&self, parameters: &[String], arguments: &[Type]) -> Type {
        match self {
            Type::Parameter(name) => parameters
                .iter()
                .position(|parameter| parameter == name)
                .and_then(|index| arguments.get(index))
                .cloned()
                .unwrap_or_else(|| self.clone()),
            Type::Refined(base, predicate) => Type::Refined(
                Box::new(base.substitute(parameters, arguments)),
                predicate.clone(),
            ),
            Type::List(item) => Type::List(Box::new(item.substitute(parameters, arguments))),
            Type::Map(value) => Type::Map(Box::new(value.substitute(parameters, arguments))),
            Type::Generic(index, inner) => Type::Generic(
                *index,
                inner
                    .iter()
                    .map(|argument| argument.substitute(parameters, arguments))
                    .collect(),
            ),
            _ => self.clone(),
        }
    }

    /// The type parameters it mentions, each once, in the order met.
    pub fn parameters(&self) -> Vec<&str> {
        fn walk<'t>(r#type: &'t Type, found: &mut Vec<&'t str>) {
            match r#type {
                Type::Parameter(name) if !found.contains(&name.as_str()) => found.push(name),
                Type::List(item) | Type::Map(item) | Type::Refined(item, _) => walk(item, found),
                Type::Generic(_, arguments) => {
                    arguments.iter().for_each(|argument| walk(argument, found))
                }
                _ => {}
            }
        }
        let mut found = Vec::new();
        walk(self, &mut found);
        found
    }
}

impl TypeKind {
    /// `self` with each of `parameters` replaced by the argument at its
    /// place in `arguments`, in every field or variant.
    pub fn substitute(&self, parameters: &[String], arguments: &[Type]) -> TypeKind {
        match self {
            TypeKind::Record(fields) => TypeKind::Record(
                fields
                    .iter()
                    .map(|field| Field {
                        r#type: field.r#type.substitute(parameters, arguments),
                        ..field.clone()
                    })
                    .collect(),
            ),
            TypeKind::Choice(variants) => TypeKind::Choice(
                variants
                    .iter()
                    .map(|variant| Shaped {
                        doc: variant.doc.clone(),
                        shape: Shape {
                            text: variant.shape.text.clone(),
                            parameters: variant
                                .shape
                                .parameters
                                .iter()
                                .map(|parameter| Parameter {
                                    name: parameter.name.clone(),
                                    r#type: parameter.r#type.substitute(parameters, arguments),
                                })
                                .collect(),
                        },
                    })
                    .collect(),
            ),
            TypeKind::Distinct(underlying) => {
                TypeKind::Distinct(underlying.substitute(parameters, arguments))
            }
        }
    }
}

/// The words of the built-in call every server answers with the guide to
/// its schema, without how to send a request: `SCHEMA`. All-caps words
/// belong to the schema language, so no method of a schema is called that.
pub const INTROSPECTION: &str = "SCHEMA";

/// The built-in `SCHEMA` method: no parameters, returning the guide as
/// `Text`, with no errors.
pub fn introspection() -> &'static Method {
    static METHOD: std::sync::LazyLock<Method> = std::sync::LazyLock::new(|| Method {
        doc: Some("The guide to this protocol and its schema.".to_string()),
        shape: Shape {
            text: INTROSPECTION.to_string(),
            parameters: Vec::new(),
        },
        returns: Some(Parameter {
            name: "guide".to_string(),
            r#type: Type::Text,
        }),
        errors: Vec::new(),
        examples: Vec::new(),
        effect: Effect::ReadOnly,
        undone_by: None,
    });
    &METHOD
}

/// `keyword` before the type `name`, as the formatter writes it: with a
/// `:` only where `name` starts with a keyword it would join, as in
/// `LIST: LIST Text`, and without one otherwise, `LIST Text`.
fn keyword_before(keyword: &str, name: String) -> String {
    match name.split(' ').next().is_some_and(anyx::is_keyword) {
        true => format!("{keyword}: {name}"),
        false => format!("{keyword} {name}"),
    }
}

/// A decoded file and the indices of its direct imports.
pub(crate) struct DecodedFile {
    pub imports: Vec<usize>,
    pub document: ast::Document,
}

impl Schema {
    /// Checks a single source, including a headerless library. Imports require
    /// [`Schema::load`], which supplies the file reader.
    pub fn parse(source: &str) -> Result<Schema, Vec<Diagnostic>> {
        Self::load("schema.protocol.anyx", |path| {
            if path == "schema.protocol.anyx" {
                Ok(source.to_string())
            } else {
                Err(format!("no reader supplied for `{path}`; use Schema::load"))
            }
        })
    }

    /// Checks an entry file and exactly the files reached through `USE`.
    /// The reader receives normalized paths relative to the caller's working
    /// directory (or absolute paths when the entry is absolute). Each file is
    /// read once. Libraries may be checked without a `PROTOCOL` header.
    pub fn load<E: std::fmt::Display>(
        entry: &str,
        mut read: impl FnMut(&str) -> Result<String, E>,
    ) -> Result<Schema, Vec<Diagnostic>> {
        let entry = normalize_path(entry);
        let mut loader = Loader {
            read: &mut read,
            files: Vec::new(),
            decoded: Vec::new(),
            visiting: Vec::new(),
            attempted: std::collections::BTreeSet::new(),
            diagnostics: Vec::new(),
        };
        loader.visit(&entry);
        if !loader.diagnostics.is_empty() {
            return Err(loader.diagnostics);
        }
        let mut checker = Checker::default();
        let schema = checker.schema(loader.files, &loader.decoded);
        let mut diagnostics = checker.diagnostics;
        diagnostics.sort_by_key(|diagnostic| {
            (
                schema
                    .files
                    .iter()
                    .position(|file| file.path == diagnostic.path),
                diagnostic.span.start,
                diagnostic.message.clone(),
            )
        });
        diagnostics.dedup();
        if diagnostics.is_empty() {
            Ok(schema)
        } else {
            Err(diagnostics)
        }
    }

    /// Whether this entry declares an endpoint rather than a library.
    pub fn is_protocol(&self) -> bool {
        !self.name.is_empty()
    }

    /// `shape` with each slot written as its parameter and type:
    /// `move (path: Text) to (new path: Text)`.
    pub fn typed_shape(&self, shape: &Shape) -> String {
        let mut parameters = shape.parameters.iter();
        shape
            .text
            .split(' ')
            .map(|word| match word {
                "_" => parameters.next().map_or_else(
                    || "_".to_string(),
                    |parameter| {
                        format!(
                            "({}: {})",
                            parameter.name,
                            self.type_name(&parameter.r#type)
                        )
                    },
                ),
                word => word.to_string(),
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// A type's plain protocol name, filling the slots of generic shapes.
    pub fn type_name(&self, r#type: &Type) -> String {
        match r#type {
            Type::Text
            | Type::Bytes
            | Type::Integer
            | Type::Float
            | Type::Boolean
            | Type::Instant
            | Type::Duration => crate::prelude::of(r#type)
                .map_or_else(|| format!("{type:?}"), |intrinsic| intrinsic.name.clone()),
            Type::Refined(base, predicate) => {
                format!("({}) WHERE [{}]", self.type_name(base), predicate.source)
            }
            Type::List(item) => keyword_before("LIST", self.type_name(item)),
            Type::Map(value) => keyword_before("MAP", self.type_name(value)),
            Type::Named(index) => {
                let definition = &self.types[*index];
                definition.name.clone()
            }
            Type::Generic(index, arguments) => {
                let definition = &self.types[*index];
                let arguments: Vec<String> = arguments
                    .iter()
                    .map(|argument| self.type_name(argument))
                    .collect();
                fill_slots(&definition.name, &arguments)
            }
            Type::Parameter(name) => name.clone(),
        }
    }

    /// The record or choice that `type` names, after reading past named
    /// types, and its fields or variants: for a generic type, with its
    /// arguments in place of its parameters, so that a value of
    /// `page of (Text)` reads like a record whose `items` are
    /// `LIST Text`. `None` for any other type.
    pub fn instance<'s>(
        &'s self,
        r#type: &'s Type,
    ) -> Option<(&'s TypeDefinition, std::borrow::Cow<'s, TypeKind>)> {
        match self.underlying(r#type) {
            Type::Named(index) => {
                let definition = &self.types[*index];
                Some((definition, std::borrow::Cow::Borrowed(&definition.kind)))
            }
            Type::Generic(index, arguments) => {
                let definition = &self.types[*index];
                Some((
                    definition,
                    std::borrow::Cow::Owned(
                        definition
                            .kind
                            .substitute(&definition.parameters, arguments),
                    ),
                ))
            }
            _ => None,
        }
    }

    /// `type` with every named type (`TYPE File Id: Text`) it stands for
    /// replaced by the type underneath, which is how the wire and JSON
    /// write it: `Text` for `file id`.
    pub fn underlying<'t>(&'t self, r#type: &'t Type) -> &'t Type {
        let mut underlying = r#type;
        // The checker rejects named types that lead back to themselves, so
        // named transitions are bounded; inline refinements are finite trees.
        let mut named = 0;
        loop {
            match underlying {
                Type::Refined(base, _) => underlying = base,
                Type::Named(index) => {
                    named += 1;
                    if named > self.types.len() {
                        break;
                    }
                    match &self.types[*index].kind {
                        TypeKind::Distinct(inner) => underlying = inner,
                        TypeKind::Record(_) | TypeKind::Choice(_) => break,
                    }
                }
                _ => break,
            }
        }
        underlying
    }

    /// The method a request calls by `full_name`, such as `files/read _`,
    /// or `SCHEMA`, the built-in one.
    pub fn method(&self, full_name: &str) -> Option<&Method> {
        self.methods
            .iter()
            .chain([introspection()])
            .find(|method| method.full_name() == full_name)
    }

    /// Whether another method has the same shape: an
    /// overload, which a call chooses by its arguments' types.
    pub fn is_overloaded(&self, method: &Method) -> bool {
        self.methods
            .iter()
            .filter(|other| other.shape.text == method.shape.text)
            .nth(1)
            .is_some()
    }

    /// `method`'s full name with each slot holding its parameter's type, as
    /// written in the flat namespace: `tag (LIST Text)`, `open (File Id)`.
    pub fn typed_name(&self, method: &Method) -> String {
        let types: Vec<String> = method
            .shape
            .parameters
            .iter()
            .map(|parameter| self.type_name(&parameter.r#type))
            .collect();
        fill_slots(&method.shape.text, &types)
    }

    /// What JSON and the generated TypeScript call `method` by: its full
    /// name, `tag _`, or its typed name, `tag (LIST Text)`, when it is
    /// overloaded.
    pub fn call_name(&self, method: &Method) -> String {
        match self.is_overloaded(method) {
            true => self.typed_name(method),
            false => method.full_name(),
        }
    }

    /// The index of the built-in `SCHEMA` call, one past the schema's own
    /// methods, as a [`crate::Call`]'s `method` holds it.
    pub fn introspection(&self) -> usize {
        self.methods.len()
    }

    /// The method at `index`: one of the schema's own, or the built-in
    /// `SCHEMA` one past them.
    pub fn method_at(&self, index: usize) -> &Method {
        match index == self.introspection() {
            true => introspection(),
            false => &self.methods[index],
        }
    }

    /// The errors `method` may return.
    pub fn errors_of(&self, method: &Method) -> Vec<&Shaped> {
        method
            .errors
            .iter()
            .map(|index| &self.errors[*index].error)
            .collect()
    }
}

/// Lexical path normalization also works in WebAssembly without a filesystem.
fn normalize_path(path: &str) -> String {
    let mut parts = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." if parts.last().is_some_and(|last| *last != "..") => {
                parts.pop();
            }
            ".." if path.starts_with('/') => {}
            _ => parts.push(part),
        }
    }
    format!(
        "{}{}",
        if path.starts_with('/') { "/" } else { "" },
        parts.join("/")
    )
}

struct Loader<'a, F> {
    read: &'a mut F,
    files: Vec<SourceFile>,
    decoded: Vec<DecodedFile>,
    visiting: Vec<String>,
    attempted: std::collections::BTreeSet<String>,
    diagnostics: Vec<Diagnostic>,
}

impl<E: std::fmt::Display, F: FnMut(&str) -> Result<String, E>> Loader<'_, F> {
    fn visit(&mut self, path: &str) -> Option<usize> {
        if let Some(index) = self.files.iter().position(|file| file.path == path) {
            return Some(index);
        }
        if !self.attempted.insert(path.to_string()) {
            return None;
        }
        let source = match (self.read)(path) {
            Ok(source) => source,
            Err(error) => {
                self.diagnostics.push(Diagnostic {
                    path: path.to_string(),
                    span: Span::empty(0),
                    message: format!("cannot read `{path}`: {error}"),
                });
                return None;
            }
        };
        let document = match decode_file(&SchemaFile {
            path,
            source: &source,
        }) {
            Ok(document) => document,
            Err(errors) => {
                self.diagnostics.extend(errors);
                return None;
            }
        };
        let imports: Vec<_> = document
            .definition
            .iter()
            .filter_map(|definition| {
                if let ast::Definition::Import(import) = definition {
                    Some(import.clone())
                } else {
                    None
                }
            })
            .collect();
        let index = self.files.len();
        self.files.push(SourceFile {
            path: path.to_string(),
            source,
        });
        self.decoded.push(DecodedFile {
            imports: Vec::new(),
            document,
        });
        self.visiting.push(path.to_string());
        for import in imports {
            let relative = &import.path.value.value;
            let error = |message| Diagnostic {
                path: path.to_string(),
                span: import.span,
                message,
            };
            if relative.is_empty()
                || relative.starts_with('/')
                || relative.contains('\\')
                || relative.contains(':')
            {
                self.diagnostics.push(error(format!(
                    "expected a relative file path in `USE`, found `{relative}`"
                )));
                continue;
            }
            let parent = path.rfind('/').map_or("", |slash| &path[..=slash]);
            let target = normalize_path(&format!("{parent}{relative}"));
            if let Some(start) = self.visiting.iter().position(|path| *path == target) {
                let mut cycle = self.visiting[start..].to_vec();
                cycle.push(target);
                self.diagnostics.push(error(format!(
                    "`USE \"{relative}\"` makes a circular import: {}",
                    cycle.join(" → ")
                )));
                continue;
            }
            let before = self.diagnostics.len();
            if let Some(imported) = self.visit(&target) {
                if self.decoded[imported].document.header.is_some() {
                    self.diagnostics.push(error(format!("cannot import `{target}`: it has a `PROTOCOL` header; move shared declarations into a headerless library")));
                } else if !self.decoded[index].imports.contains(&imported) {
                    self.decoded[index].imports.push(imported);
                }
            } else if self.diagnostics.len() > before {
                // Report read failures at USE; syntax errors keep their own file.
                if self.diagnostics[before].message.starts_with("cannot read") {
                    let message = self.diagnostics.remove(before).message;
                    self.diagnostics.push(error(message));
                }
            }
        }
        self.visiting.pop();
        Some(index)
    }
}

/// Parses `file` and decodes it with the schema language's grammar.
fn decode_file(file: &SchemaFile) -> Result<ast::Document, Vec<Diagnostic>> {
    let found = |span: Span, message: String| Diagnostic {
        path: file.path.to_string(),
        span,
        message,
    };
    let parsed = anyx::parse(file.source);
    if !parsed.errors.is_empty() {
        return Err(parsed
            .errors
            .into_iter()
            .map(|error| found(error.span, error.message))
            .collect());
    }
    let nodes = parsed.syntax.nodes();
    ast::decode_document(&nodes).map_err(|error| {
        if let Some(shape) = slashed_method(&nodes, error.span) {
            let name = &file.source[shape.start..shape.end];
            return vec![found(
                shape,
                format!(
                    "found the method `{name}`, with `/` in its name; expected words alone, method names are flat: write `METHOD read` and call it as `read`"
                ),
            )];
        }
        if let Some((span, intrinsic)) = old_built_in(&nodes, error.span) {
            return vec![found(
                span,
                format!(
                    "found `{old}`; expected `{new}`, its name since the built-in types are declared in the prelude: type names are written in Title Case, and all-caps words belong to the schema language",
                    old = &file.source[span.start..span.end],
                    new = intrinsic.name
                ),
            )];
        }
        // The definition the error is in, since `INTRINSIC METHOD` fails at
        // `METHOD`, after the `INTRINSIC` that caused it.
        let definition = nodes
            .iter()
            .find(|node| node.span.start <= error.span.start && error.span.start < node.span.end)
            .map_or(error.span, |node| node.span);
        if let Some(span) = word_within(&nodes, definition, "INTRINSIC") {
            return vec![found(
                span,
                "found `INTRINSIC` in a schema; expected it only in the prelude, which declares the built-in types, since the implementation supplies what an `INTRINSIC` declaration means; remove it".to_string(),
            )];
        }
        if let Some(default) = default_outside_record(&nodes, error.span) {
            return vec![found(
                default,
                "found a `DEFAULT` outside a `RECORD`; expected one only on an `OPTIONAL` field of a `RECORD`, as in `limit: OPTIONAL: Integer DEFAULT 50`, since a parameter is always written".to_string(),
            )];
        }
        let message = match (unnamed_return(&nodes, error.span), lone_error(&nodes, error.span)) {
            (true, _) => "found a `RETURNS` without a name; expected `RETURNS (name: TYPE)`, such as `RETURNS (new revision: Integer)`, since the name says what the value is".to_string(),
            (_, true) => "found an `ERROR` on its own; expected each error in the brackets of the method that has it, `METHOD read (path: Text) RETURNS (file: File) ERRORS [missing]`, or of an `ERRORS` group that methods share by joining it with `+`: `ERRORS file errors [trashed, missing]`".to_string(),
            _ => error.message(),
        };
        vec![found(error.span, message)]
    })
}

/// The argument right after `word` in a top-level call that has it, if
/// `span` is inside it, such as the name of a `METHOD`. Only a slot that
/// comes straight after the word counts: in `METHOD _ RETURNS NOTHING
/// ERRORS _`, nothing comes after `RETURNS`, and the `ERRORS` list belongs
/// to `ERRORS`.
fn after_word<'a>(nodes: &'a [Node], span: Span, word: &str) -> Option<&'a Node> {
    nodes.iter().find_map(|node| {
        let NodeKind::Call(call) = &node.kind else {
            return None;
        };
        let mut slot = 0;
        call.name
            .iter()
            .enumerate()
            .find_map(|(index, part)| match part {
                NamePart::Slot => {
                    slot += 1;
                    None
                }
                NamePart::Word { text, .. }
                    if text == word && call.name.get(index + 1) == Some(&NamePart::Slot) =>
                {
                    call.arguments.get(slot).filter(|after| {
                        after.span.start <= span.start && span.end <= after.span.end
                    })
                }
                NamePart::Word { .. } => None,
            })
    })
}

/// Each word of a call's name within `span`, among `nodes` and the nodes
/// they hold, with its span.
fn words_within(nodes: &[Node], span: Span) -> Vec<(&str, Span)> {
    fn walk<'n>(node: &'n Node, span: Span, found: &mut Vec<(&'n str, Span)>) {
        if node.span.end <= span.start || span.end <= node.span.start {
            return;
        }
        let children: &[Node] = match &node.kind {
            NodeKind::Call(call) => {
                for part in &call.name {
                    if let NamePart::Word { text, span: at } = part
                        && span.start <= at.start
                        && at.end <= span.end
                    {
                        found.push((text, *at));
                    }
                }
                &call.arguments
            }
            NodeKind::Parens(nodes) | NodeKind::Brackets(nodes) | NodeKind::Braces(nodes) => nodes,
            _ => &[],
        };
        children.iter().for_each(|child| walk(child, span, found));
    }
    let mut found = Vec::new();
    nodes.iter().for_each(|node| walk(node, span, &mut found));
    found.sort_by_key(|(_, at)| at.start);
    found
}

/// The first all-caps name of a built-in type from before the prelude,
/// such as `INTEGER`, within `span`, and the type it is now.
fn old_built_in(nodes: &[Node], span: Span) -> Option<(Span, &'static crate::prelude::Intrinsic)> {
    words_within(nodes, span)
        .into_iter()
        .find_map(|(text, at)| crate::prelude::renamed(text).map(|intrinsic| (at, intrinsic)))
}

/// Where `word` first is within `span`.
fn word_within(nodes: &[Node], span: Span, word: &str) -> Option<Span> {
    words_within(nodes, span)
        .into_iter()
        .find(|(text, _)| *text == word)
        .map(|(_, at)| at)
}

/// Whether `span` is inside what a `METHOD` returns, written without a name:
/// `RETURNS (Text)` rather than `RETURNS (text: Text)`.
fn unnamed_return(nodes: &[Node], span: Span) -> bool {
    after_word(nodes, span, "RETURNS").is_some_and(|returned| {
        !matches!(&returned.kind, NodeKind::Call(call)
            if call.name.iter().any(|part| matches!(part, NamePart::Word { text, .. } if !anyx::is_keyword(text)))
                && call.name.contains(&NamePart::Slot))
    })
}

/// Where the call with a `DEFAULT` that `span` is in is, when it is not in
/// a `RECORD`, as in a parameter `(limit: Integer DEFAULT 50)`.
fn default_outside_record(nodes: &[Node], span: Span) -> Option<Span> {
    fn has_word(node: &Node, word: &str) -> bool {
        matches!(&node.kind, NodeKind::Call(call)
            if call.name.iter().any(|part| matches!(part, NamePart::Word { text, .. } if text == word)))
    }
    fn within(node: &Node, span: Span) -> Option<Span> {
        if !(node.span.start <= span.start && span.end <= node.span.end) {
            return None;
        }
        let children: &[Node] = match &node.kind {
            NodeKind::Call(call) => &call.arguments,
            NodeKind::Parens(nodes) | NodeKind::Brackets(nodes) | NodeKind::Braces(nodes) => nodes,
            _ => &[],
        };
        children
            .iter()
            .find_map(|child| within(child, span))
            .or_else(|| has_word(node, "DEFAULT").then_some(node.span))
    }
    nodes
        .iter()
        .filter(|node| !has_word(node, "RECORD"))
        .find_map(|node| within(node, span))
}

/// Whether `span` is in a top-level `ERROR`, which a schema no longer has:
/// errors are defined in the brackets of a method or an `ERRORS` group.
fn lone_error(nodes: &[Node], span: Span) -> bool {
    nodes.iter().any(|node| {
        node.span.start <= span.start
            && span.end <= node.span.end
            && matches!(&node.kind, NodeKind::Call(call)
                if call.name.iter().any(|part| matches!(part, NamePart::Word { text, .. } if text == "ERROR")))
    })
}

/// Where the name of the `METHOD` that `span` is in is, if it has a `/`, as
/// in `METHOD files/read (path: Text)`.
fn slashed_method(nodes: &[Node], span: Span) -> Option<Span> {
    let shape = after_word(nodes, span, "METHOD")?;
    let NodeKind::Call(call) = &shape.kind else {
        return None;
    };
    call.name
        .iter()
        .any(|part| matches!(part, NamePart::Word { text, .. } if text == "/"))
        .then_some(shape.span)
}

/// Something wrong in a schema, and where.
#[derive(Clone, Debug, PartialEq)]
pub struct Diagnostic {
    /// The normalized path of the source file.
    pub path: String,
    pub span: Span,
    pub message: String,
}

impl Diagnostic {
    /// `file:line:column: message`, then the line with the span underlined.
    /// `source` is the file at [`Diagnostic::path`].
    pub fn render(&self, source: &str, file: &str) -> String {
        let position = line_column(source, self.span.start);
        let line = source.lines().nth(position.line - 1).unwrap_or("");
        let start = self.span.start.min(source.len());
        let width = source[start..self.span.end.clamp(start, source.len())]
            .lines()
            .next()
            .map_or(1, |text| text.chars().count().max(1));
        format!(
            "{file}:{}:{}: {}\n  {line}\n  {}{}",
            position.line,
            position.column,
            self.message,
            " ".repeat(position.column - 1),
            "^".repeat(width)
        )
    }
}
