//! A checked schema: every name resolved, and every type, field, variant,
//! error and method defined once in its scope.
//!
//! A schema is one file, or a folder of them. In a folder, `protocol.anyx`
//! holds the `PROTOCOL` header alone, and every other file is a scope named
//! by its path: `files.anyx` is the scope `files`, and `files/export.anyx`
//! the scope `files/export`. A method in a scope is called by its full name,
//! `files/export/midi _`. A type or an error is private to its scope unless
//! it is `PUBLIC`, and another scope refers to it by its full path,
//! `common/diagnostic`.

use anyx::{NamePart, Node, NodeKind, Span, line_column};

use crate::check::Checker;
use crate::codec::Outcome;
use crate::grammar as ast;
use crate::value::Value;

/// The file of a schema's folder that holds its `PROTOCOL` header, and the
/// name a schema of one file is read under.
pub const ROOT_FILE: &str = "protocol.anyx";

const EXTENSION: &str = ".anyx";

/// One file of a schema, as given to [`Schema::parse_files`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SchemaFile<'a> {
    /// Relative to the schema's folder, with `/` between folders:
    /// `files/export.anyx`.
    pub path: &'a str,
    pub source: &'a str,
}

/// A schema that passed every check.
#[derive(Clone, Debug, PartialEq)]
pub struct Schema {
    /// The files as written, which the guide shows as they are: the root
    /// first, then each scope in order of its path.
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
    /// The scope it declares: empty for the root.
    pub scope: Vec<String>,
    pub source: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TypeDefinition {
    /// The scope it is declared in: empty for the root.
    pub scope: Vec<String>,
    /// `PUBLIC`: other scopes may use it.
    pub public: bool,
    pub doc: Option<String>,
    /// Lowercase words, such as `file summary`.
    pub name: String,
    /// The type parameters of a `GENERIC` record or choice, in the order
    /// written, `GENERIC (value, problem)`; empty for any other type.
    pub parameters: Vec<String>,
    pub kind: TypeKind,
}

#[derive(Clone, Debug, PartialEq)]
pub enum TypeKind {
    Record(Vec<Field>),
    /// Variants, each written in its own shape.
    Choice(Vec<Shaped>),
    /// A named type, `TYPE file id: TEXT`: the type underneath, under a name
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
/// `conflict at revision (revision: INTEGER)`.
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
/// On the wire it is written in its shape alone, without its scope.
#[derive(Clone, Debug, PartialEq)]
pub struct ErrorDefinition {
    pub scope: Vec<String>,
    pub public: bool,
    pub error: Shaped,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Method {
    /// The scope it is declared in, which its full name starts with.
    pub scope: Vec<String>,
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

/// `UNDONE BY move (new path: TEXT) to (path: TEXT)`: the call of another
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
    /// `move (new path: TEXT) to (path: TEXT)`.
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
    /// What a request calls it by: its scope's path and its shape,
    /// `files/export/midi _`, or its shape alone in the root.
    pub fn full_name(&self) -> String {
        scoped(&self.scope, &self.shape.text)
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
    Number,
    Boolean,
    /// A point in time, to the millisecond. On the wire, RFC 3339 text in
    /// UTC: `"2026-11-14T06:00:08.000Z"`.
    Instant,
    /// A length of time, to the millisecond. On the wire, whole numbers
    /// with their units: `1 hour 30 minutes`.
    Duration,
    List(Box<Type>),
    /// Text keys, each to a value of the type. On the wire, braces of keys
    /// and values: `{ "hymn" 3, "ode" 1 }`.
    Map(Box<Type>),
    /// The index of a type in [`Schema::types`], one that isn't generic.
    Named(usize),
    /// A generic record or choice with its arguments, `page WITH (item:
    /// TEXT)`: the index of the type in [`Schema::types`], and a type for
    /// each of its parameters, in the order of its parameters.
    Generic(usize, Vec<Type>),
    /// A type parameter, by its name, within the generic record or choice
    /// that declares it. A use of that type puts its argument in its place,
    /// so a value is never of this type.
    Parameter(String),
}

impl Type {
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
                Type::List(item) | Type::Map(item) => walk(item, found),
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
/// `TEXT`, with no errors.
pub fn introspection() -> &'static Method {
    static METHOD: std::sync::LazyLock<Method> = std::sync::LazyLock::new(|| Method {
        scope: Vec::new(),
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

/// `name` behind the path of `scope`: `common/diagnostic`, or `name` alone
/// in the root.
pub fn scoped(scope: &[String], name: &str) -> String {
    scope
        .iter()
        .map(String::as_str)
        .chain([name])
        .collect::<Vec<_>>()
        .join("/")
}

/// The file that declares `scope`: `files/export.anyx`.
pub fn scope_file(scope: &[String]) -> String {
    match scope.is_empty() {
        true => ROOT_FILE.to_string(),
        false => format!("{}{EXTENSION}", scope.join("/")),
    }
}

/// A file with the scope its path declares.
struct ScopedFile<'a> {
    scope: Vec<String>,
    file: SchemaFile<'a>,
}

/// A file read and decoded, for the checker.
pub(crate) struct DecodedFile {
    pub scope: Vec<String>,
    pub document: ast::Document,
}

impl Schema {
    /// Reads and checks a schema of one file.
    pub fn parse(source: &str) -> Result<Schema, Vec<Diagnostic>> {
        Schema::parse_files(&[SchemaFile {
            path: ROOT_FILE,
            source,
        }])
    }

    /// Reads and checks a schema of a folder of files: `protocol.anyx` and
    /// a file per scope.
    pub fn parse_files(files: &[SchemaFile]) -> Result<Schema, Vec<Diagnostic>> {
        let mut scoped_files = files
            .iter()
            .map(|file| file_scope(files, file).map(|scope| ScopedFile { scope, file: *file }))
            .collect::<Result<Vec<_>, _>>()?;
        scoped_files.sort_by(|left, right| left.scope.cmp(&right.scope));
        let mut decoded = Vec::new();
        let mut diagnostics = Vec::new();
        for scoped in &scoped_files {
            match decode_file(&scoped.file) {
                Ok(document) => decoded.push(DecodedFile {
                    scope: scoped.scope.clone(),
                    document,
                }),
                Err(found) => diagnostics.extend(found),
            }
        }
        if !diagnostics.is_empty() {
            return Err(diagnostics);
        }
        let sources = scoped_files
            .iter()
            .map(|scoped| SourceFile {
                path: scoped.file.path.to_string(),
                scope: scoped.scope.clone(),
                source: scoped.file.source.to_string(),
            })
            .collect();
        let mut checker = Checker::default();
        let schema = checker.schema(sources, &decoded);
        let mut diagnostics = checker.diagnostics;
        if diagnostics.is_empty() {
            return Ok(schema);
        }
        diagnostics.sort_by_key(|diagnostic| {
            (
                scoped_files
                    .iter()
                    .position(|scoped| scoped.file.path == diagnostic.path),
                diagnostic.span.start,
                diagnostic.message.clone(),
            )
        });
        diagnostics.dedup();
        Err(diagnostics)
    }

    /// `shape` with each slot written as its parameter and type:
    /// `move (path: TEXT) to (new path: TEXT)`.
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

    pub fn type_name(&self, r#type: &Type) -> String {
        match r#type {
            Type::Text => "TEXT".to_string(),
            Type::Bytes => "BYTES".to_string(),
            Type::Integer => "INTEGER".to_string(),
            Type::Number => "NUMBER".to_string(),
            Type::Boolean => "BOOLEAN".to_string(),
            Type::Instant => "INSTANT".to_string(),
            Type::Duration => "DURATION".to_string(),
            Type::List(item) => format!("LIST: {}", self.type_name(item)),
            Type::Map(value) => format!("MAP: {}", self.type_name(value)),
            Type::Named(index) => {
                let definition = &self.types[*index];
                scoped(&definition.scope, &definition.name)
            }
            Type::Generic(index, arguments) => {
                let definition = &self.types[*index];
                let arguments: Vec<String> = definition
                    .parameters
                    .iter()
                    .zip(arguments)
                    .map(|(parameter, argument)| {
                        format!("{parameter}: {}", self.type_name(argument))
                    })
                    .collect();
                format!(
                    "{} WITH ({})",
                    scoped(&definition.scope, &definition.name),
                    arguments.join(", ")
                )
            }
            Type::Parameter(name) => name.clone(),
        }
    }

    /// The record or choice that `type` names, after reading past named
    /// types, and its fields or variants: for a generic type, with its
    /// arguments in place of its parameters, so that a value of
    /// `page WITH (item: TEXT)` reads like a record whose `items` are
    /// `LIST: TEXT`. `None` for any other type.
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

    /// `type` with every named type (`TYPE file id: TEXT`) it stands for
    /// replaced by the type underneath, which is how the wire and JSON
    /// write it: `TEXT` for `file id`.
    pub fn underlying<'t>(&'t self, r#type: &'t Type) -> &'t Type {
        let mut underlying = r#type;
        // The checker rejects named types that lead back to themselves, so
        // this ends within as many steps as there are types.
        for _ in 0..=self.types.len() {
            match underlying {
                Type::Named(index) => match &self.types[*index].kind {
                    TypeKind::Distinct(inner) => underlying = inner,
                    TypeKind::Record(_) | TypeKind::Choice(_) => break,
                },
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

    /// Every scope, each after the scope it is in, from the root: the scopes
    /// of the files, and the folders they are in.
    pub fn scopes(&self) -> Vec<Vec<String>> {
        let mut scopes: Vec<Vec<String>> = self
            .files
            .iter()
            .flat_map(|file| (0..=file.scope.len()).map(|length| file.scope[..length].to_vec()))
            .collect();
        scopes.sort();
        scopes.dedup();
        scopes
    }
}

/// The scope `file` declares, from its path.
fn file_scope(files: &[SchemaFile], file: &SchemaFile) -> Result<Vec<String>, Vec<Diagnostic>> {
    let at_start = |path: &str, message: String| {
        vec![Diagnostic {
            path: path.to_string(),
            span: Span::empty(0),
            message,
        }]
    };
    if files.iter().filter(|other| other.path == file.path).count() > 1 {
        return Err(at_start(
            file.path,
            format!(
                "found `{}` twice among the schema's files; expected each path once",
                file.path
            ),
        ));
    }
    if !files.iter().any(|other| other.path == ROOT_FILE) {
        return Err(at_start(
            ROOT_FILE,
            format!(
                "found no `{ROOT_FILE}` among the schema's files; expected it, with the `PROTOCOL` header"
            ),
        ));
    }
    if file.path == ROOT_FILE {
        return Ok(Vec::new());
    }
    let segments: Option<Vec<String>> = file
        .path
        .strip_suffix(EXTENSION)
        .map(|stem| stem.split('/').map(str::to_string).collect());
    match segments {
        Some(segments) if segments.iter().all(|segment| is_scope_word(segment)) => Ok(segments),
        _ => Err(at_start(
            file.path,
            format!(
                "found the schema file `{}`; expected a path of single lowercase words, each of ASCII letters and digits starting with a letter, ending in `{EXTENSION}`, such as `files/export{EXTENSION}`, since each word names a scope that requests write",
                file.path
            ),
        )),
    }
}

fn is_scope_word(word: &str) -> bool {
    word.starts_with(|c: char| c.is_ascii_lowercase())
        && word
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
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
                    "found the method `{name}`, with `/` in its name; expected words alone, since a method's scope is its file: `METHOD read` in `files.anyx` is called as `files/read`"
                ),
            )];
        }
        if let Some(keyword) = keyword_before_with(&nodes, error.span) {
            return vec![found(
                error.span,
                format!(
                    "found `WITH` in the call of `{keyword}`; expected a `:` after `{keyword}`, as in `{keyword}: page WITH (item: TEXT)`, since the keywords of a run belong to one call"
                ),
            )];
        }
        if let Some(default) = default_outside_record(&nodes, error.span) {
            return vec![found(
                default,
                "found a `DEFAULT` outside a `RECORD`; expected one only on an `OPTIONAL` field of a `RECORD`, as in `limit: OPTIONAL: INTEGER DEFAULT 50`, since a parameter is always written".to_string(),
            )];
        }
        let message = match (unnamed_return(&nodes, error.span), lone_error(&nodes, error.span)) {
            (true, _) => "found a `RETURNS` without a name; expected `RETURNS (name: TYPE)`, such as `RETURNS (new revision: INTEGER)`, since the name says what the value is".to_string(),
            (_, true) => "found an `ERROR` on its own; expected each error in the brackets of the method that has it, `METHOD read (path: TEXT) RETURNS (file: file) ERRORS [missing]`, or of an `ERRORS` group that methods share by joining it with `+`: `ERRORS file errors [trashed, missing]`".to_string(),
            _ => error.message(),
        };
        vec![found(error.span, message)]
    })
}

/// The argument after `word` in a top-level call that has it, if `span` is
/// inside it, such as the name of a `METHOD`.
fn after_word<'a>(nodes: &'a [Node], span: Span, word: &str) -> Option<&'a Node> {
    nodes.iter().find_map(|node| {
        let NodeKind::Call(call) = &node.kind else {
            return None;
        };
        let mut slot = 0;
        call.name.iter().find_map(|part| match part {
            NamePart::Slot => {
                slot += 1;
                None
            }
            NamePart::Word { text, .. } if text == word => call
                .arguments
                .get(slot)
                .filter(|after| after.span.start <= span.start && span.end <= after.span.end),
            NamePart::Word { .. } => None,
        })
    })
}

/// Whether `span` is inside what a `METHOD` returns, written without a name:
/// `RETURNS (TEXT)` rather than `RETURNS (text: TEXT)`.
fn unnamed_return(nodes: &[Node], span: Span) -> bool {
    after_word(nodes, span, "RETURNS").is_some_and(|returned| {
        !matches!(&returned.kind, NodeKind::Call(call)
            if call.name.iter().any(|part| matches!(part, NamePart::Word { text, .. } if !anyx::is_keyword(text)))
                && call.name.contains(&NamePart::Slot))
    })
}

/// Where the call with a `DEFAULT` that `span` is in is, when it is not in
/// a `RECORD`, as in a parameter `(limit: INTEGER DEFAULT 50)`.
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

/// The keyword before a `WITH` at `span` in the same call, as in
/// `LIST page WITH (item: TEXT)`, which needs a `:` after `LIST`.
fn keyword_before_with(nodes: &[Node], span: Span) -> Option<&'static str> {
    nodes.iter().find_map(|node| {
        if !(node.span.start <= span.start && span.end <= node.span.end) {
            return None;
        }
        let children: &[Node] = match &node.kind {
            NodeKind::Call(call) => {
                let at_with = call.name.iter().any(|part| {
                    matches!(part, NamePart::Word { text, span: at } if text == "WITH" && *at == span)
                });
                let keyword = ["LIST", "MAP", "OPTIONAL"].into_iter().find(|keyword| {
                    call.name
                        .iter()
                        .any(|part| matches!(part, NamePart::Word { text, .. } if text == keyword))
                });
                if let (true, Some(keyword)) = (at_with, keyword) {
                    return Some(keyword);
                }
                &call.arguments
            }
            NodeKind::Parens(nodes) | NodeKind::Brackets(nodes) | NodeKind::Braces(nodes) => nodes,
            _ => &[],
        };
        keyword_before_with(children, span)
    })
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
/// in `METHOD files/read (path: TEXT)`.
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
    /// The file it is in, as given to [`Schema::parse_files`], or
    /// [`ROOT_FILE`] for a schema of one file.
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
