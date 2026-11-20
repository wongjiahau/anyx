//! Checks flat names, direct import visibility, types and method shapes.

use anyx::{NamePart, Node, NodeKind, Span, StringKind, StringLiteral};

use crate::codec;
use crate::response::{self, WrittenOutcome};

use crate::error_terms::{self, ErrorTerms, GroupReference, Misplaced};
use crate::grammar as ast;
use crate::schema::{
    DecodedFile, Diagnostic, Effect, ErrorDefinition, Example, Field, FieldForm, Method, Parameter,
    Schema, Shape, ShapePart, Shaped, SourceFile, Type, TypeDefinition, TypeKind, Undo, Written,
    code, fill_slots,
};

/// A type, error or alias, where it is declared.
struct Declared {
    name: String,
    span: Span,
    /// The index of its file.
    file: usize,
    public: bool,
}

/// An `ERRORS` group: `` ERRORS file errors [`trashed, `missing] + other ``.
struct Group<'a> {
    declared: Declared,
    /// Its own errors, as written.
    own: &'a [ast::Error],
    /// Their indices into the errors, once read.
    own_errors: Vec<usize>,
    /// The groups it joins with `+`, unresolved.
    joined: Vec<GroupReference>,
    /// Every error it stands for, once expanded: `None` when it can't be.
    expanded: Option<Option<Vec<usize>>>,
}

/// A field's `DEFAULT`, to read once every type is known.
struct PendingDefault {
    /// The index of the record's type, and of the field in it.
    r#type: usize,
    field: usize,
    /// The entry the field takes when left out, as in `age 0`.
    entry: anyx::Node,
    file: usize,
}

/// A record's fields, to check once every type is known that no way of
/// writing an entry leads to two of them.
struct PendingForms {
    /// The index of the record's type.
    r#type: usize,
    file: usize,
    /// Where each field's name is written.
    spans: Vec<Span>,
}

/// A union to check, once every type is known, for two members written
/// alike. A union written with `UNION` is checked as it is; a use of a
/// generic type or alias, for the unions it makes with its arguments.
struct PendingUnion {
    r#type: Type,
    /// The parameters to put `arguments` in place of: empty for a union
    /// checked as it is.
    parameters: Vec<String>,
    arguments: Vec<Type>,
    /// A union written with `UNION`, rather than a use.
    written: bool,
    /// Where the union, or the use, is written.
    span: Span,
    file: usize,
}

/// An error of a method, and where its list names it.
struct Listed {
    index: usize,
    span: Span,
}

/// A `TYPE ALIAS`: a name for a type, which each use stands for.
struct Alias<'a> {
    declared: Declared,
    /// Its type parameters, in the order of their slots.
    parameters: Vec<ast::Spanned<String>>,
    /// The type it stands for, as written: `None` when it is missing,
    /// which is reported where it is declared.
    written: Option<&'a ast::Type>,
    resolved: Resolved,
}

/// How far an alias's type has been read.
enum Resolved {
    No,
    /// Being read, so a use of the alias now leads back to it.
    Reading,
    /// Read, or `None` when it leads back to the alias.
    Yes(Option<Type>),
}

/// A type reference read as its plain shape.
struct Path {
    name: String,
}

/// What a `RECORD`, `CHOICE`, `TYPE` or `ERRORS` group declares, public or
/// not.
struct Declaration<'b, T> {
    doc: &'b Option<ast::Spanned<StringLiteral>>,
    /// Where `INTRINSIC` is written, which only the prelude may.
    intrinsic: Option<Span>,
    public: bool,
    body: T,
}

fn record(record: &ast::Record) -> Declaration<'_, TypeShape<'_, Vec<ast::Field>>> {
    let parts = record.body.arguments.iter().map(|part| match part {
        ast::RecordPart::Fields(fields) => Part::Body(fields),
        ast::RecordPart::TypeParameter(parameter) => Part::Parameter(parameter),
    });
    Declaration {
        doc: &record.doc,
        intrinsic: record.intrinsic,
        public: record.public.is_some(),
        body: type_shape(&record.body.name, parts),
    }
}

fn choice(choice: &ast::Choice) -> Declaration<'_, TypeShape<'_, Vec<ast::Variant>>> {
    let parts = choice.body.arguments.iter().map(|part| match part {
        ast::ChoicePart::Variants(variants) => Part::Body(variants),
        ast::ChoicePart::TypeParameter(parameter) => Part::Parameter(parameter),
    });
    Declaration {
        doc: &choice.doc,
        intrinsic: None,
        public: choice.public.is_some(),
        body: type_shape(&choice.body.name, parts),
    }
}

fn errors(errors: &ast::Errors) -> Declaration<'_, &ast::ErrorList> {
    Declaration {
        doc: &errors.doc,
        intrinsic: errors.intrinsic,
        public: errors.public.is_some(),
        body: &errors.body,
    }
}

/// A named type's constructor and the type it holds: `NAMED TYPE Range
/// from (Integer) to (Integer)` is written `Range from _ to _` and holds
/// a tuple of two `Integer`s.
fn constructor_kind(constructor: Vec<ShapePart>, mut slots: Vec<Type>) -> TypeKind {
    let underlying = if slots.len() == 1 {
        slots.remove(0)
    } else {
        Type::Tuple(slots)
    };
    TypeKind::Distinct {
        constructor,
        underlying,
    }
}

/// A `TYPE ALIAS`'s name read as a shape: `TYPE ALIAS (Item: TYPE) Pair:
/// LIST Item` is the shape `_ Pair` with the parameter `Item`, standing for
/// `LIST Item`.
struct AliasShape<'b> {
    /// The shape without the last slot, the type's: `_ Pair`, spanned from
    /// its first word to its last.
    name: ast::Spanned<String>,
    /// Its words alone: `Pair`.
    words: ast::Spanned<String>,
    parameters: Vec<&'b ast::TypeParameter>,
    /// The type it stands for, when it fills the last slot.
    r#type: Option<&'b ast::Type>,
    /// Types in any other slot, which only a type parameter may fill.
    misplaced: Vec<Span>,
}

/// An alias's shape with its parameters in their slots: `(Item) Pair`.
fn alias_shown(alias: &Alias) -> String {
    let mut parameters = alias.parameters.iter();
    alias
        .declared
        .name
        .split(' ')
        .map(|word| match word {
            "_" => format!(
                "({})",
                parameters
                    .next()
                    .map_or("_", |parameter| parameter.value.as_str())
            ),
            word => word.to_string(),
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn alias_shape(body: &ast::AliasBody) -> AliasShape<'_> {
    let mut parts: Vec<&ast::AliasPart> = body.arguments.iter().collect();
    let mut name_parts = body.name.parts.clone();
    let r#type = match (parts.last(), name_parts.last()) {
        (Some(ast::AliasPart::Type(r#type)), Some(NamePart::Slot)) => {
            parts.pop();
            name_parts.pop();
            Some(r#type)
        }
        _ => None,
    };
    let mut parameters = Vec::new();
    let mut misplaced = Vec::new();
    for part in parts {
        match part {
            ast::AliasPart::Type(r#type) => misplaced.push(r#type.span()),
            ast::AliasPart::TypeParameter(parameter) => parameters.push(parameter),
        }
    }
    AliasShape {
        name: ast::Spanned {
            value: ShapePart::text(&ShapePart::of(&name_parts)),
            span: body.name.span,
        },
        words: spanned_words(&body.name),
        parameters,
        r#type,
        misplaced,
    }
}

/// The variants of an alias of a union of variants alone, `` TYPE ALIAS
/// Case: UNION [`smart, `any] ``, which keeps its name as a type; `None`
/// for any other alias.
fn union_of_variants(alias: &ast::TypeAlias) -> Option<Vec<&ast::Variant>> {
    let Some(ast::Type::Union(union)) = alias_shape(&alias.body).r#type else {
        return None;
    };
    let variants: Option<Vec<&ast::Variant>> = union
        .members
        .value
        .iter()
        .map(|member| match member {
            ast::UnionMember::Variant(variant) => Some(variant),
            _ => None,
        })
        .collect();
    variants.filter(|variants| !variants.is_empty())
}

/// A slot of a `RECORD`'s or `CHOICE`'s name, as written: the braces of its
/// fields or variants, or a type parameter.
enum Part<'b, T> {
    Body(&'b ast::Spanned<T>),
    Parameter(&'b ast::TypeParameter),
}

/// The name of a `RECORD` or `CHOICE` read as a shape: `page of (Item: TYPE)
/// { … }` is the shape `page of _` with the parameter `item`, and its fields.
struct TypeShape<'b, T> {
    /// The shape without the slot of the braces, `page of _`, spanned from
    /// its first word to its last.
    name: ast::Spanned<String>,
    /// Its words alone: `page of`.
    words: ast::Spanned<String>,
    parameters: Vec<&'b ast::TypeParameter>,
    /// What the braces hold, when they come last; `None` when they don't.
    body: Option<&'b ast::Spanned<T>>,
    /// Braces anywhere but last, which only a type parameter may fill.
    misplaced: Vec<Span>,
}

fn type_shape<'b, T>(
    name: &ast::CallName,
    parts: impl Iterator<Item = Part<'b, T>>,
) -> TypeShape<'b, T> {
    let mut parts: Vec<Part<'b, T>> = parts.collect();
    let mut name_parts = name.parts.clone();
    let body = match (parts.last(), name_parts.last()) {
        (Some(Part::Body(body)), Some(NamePart::Slot)) => {
            let body = *body;
            parts.pop();
            name_parts.pop();
            Some(body)
        }
        _ => None,
    };
    let mut parameters = Vec::new();
    let mut misplaced = Vec::new();
    for part in parts {
        match part {
            Part::Body(braces) => misplaced.push(braces.span),
            Part::Parameter(parameter) => parameters.push(parameter),
        }
    }
    let text = name_parts
        .iter()
        .map(|part| match part {
            NamePart::Word { text, .. } => text.as_str(),
            NamePart::Slot => "_",
        })
        .collect::<Vec<_>>()
        .join(" ");
    TypeShape {
        name: ast::Spanned {
            value: text,
            span: name.span,
        },
        words: spanned_words(name),
        parameters,
        body,
        misplaced,
    }
}

/// A method's clauses.
struct MethodParts<'b> {
    doc: &'b Option<ast::Spanned<StringLiteral>>,
    shape: &'b ast::Shape,
    /// `None` for `RETURNS NOTHING`.
    returns: Option<&'b ast::Returns>,
    errors: Option<&'b ast::ErrorList>,
    undone_by: Option<&'b ast::Undo>,
    examples: Option<&'b ast::Examples>,
}

fn method_parts(method: &ast::Method) -> MethodParts<'_> {
    MethodParts {
        doc: &method.doc,
        shape: &method.shape,
        returns: match &method.returns {
            ast::MethodReturns::Value(returns) => Some(returns),
            ast::MethodReturns::Nothing(_) => None,
        },
        errors: method.errors.as_ref(),
        undone_by: method.undone_by.as_ref(),
        examples: method.examples.as_ref(),
    }
}

/// A method's `UNDONE BY`, checked once every method is read, since it may
/// name a method declared after it.
struct PendingUndo<'a> {
    /// The method's index in the schema.
    method: usize,
    file: usize,
    undo: &'a ast::Undo,
}

/// A method's `EXAMPLES`, checked once the schema they are checked against
/// is read.
struct PendingExamples<'a> {
    /// The method's index in the schema.
    method: usize,
    file: usize,
    examples: &'a ast::Examples,
}

fn doc(doc: &Option<ast::Spanned<StringLiteral>>) -> Option<String> {
    doc.as_ref().map(|doc| doc.value.value.clone())
}

fn spanned_words(name: &ast::CallName) -> ast::Spanned<String> {
    ast::Spanned {
        value: name.words().collect::<Vec<_>>().join(" "),
        span: name.span,
    }
}

#[derive(Default)]
pub(crate) struct Checker<'a> {
    paths: Vec<String>,
    prelude_file: usize,
    prelude_names: Vec<String>,
    imports: Vec<Vec<usize>>,
    sources: Vec<String>,
    /// The file being checked, which diagnostics are in.
    file: usize,
    /// Each type, in order.
    types: Vec<Declared>,
    /// The type parameters of each type, in the order of `types`: empty for
    /// a type that isn't generic.
    type_parameters: Vec<Vec<ast::Spanned<String>>>,
    /// The type parameters of the generic record or alias being checked,
    /// which its fields and variants may use as types.
    parameters: Vec<String>,
    groups: Vec<Group<'a>>,
    aliases: Vec<Alias<'a>>,
    /// Each error, in the order of the schema's errors.
    errors: Vec<Declared>,
    defaults: Vec<PendingDefault>,
    forms: Vec<PendingForms>,
    unions: Vec<PendingUnion>,
    examples: Vec<PendingExamples<'a>>,
    undos: Vec<PendingUndo<'a>>,
    pub(crate) diagnostics: Vec<Diagnostic>,
}

impl<'a> Checker<'a> {
    fn error(&mut self, span: Span, message: impl Into<String>) {
        self.diagnostics.push(Diagnostic {
            path: self.paths[self.file].clone(),
            span,
            message: message.into(),
        });
    }

    /// Checks within `file` and its declarations, then goes back.
    fn within<T>(&mut self, file: usize, check: impl FnOnce(&mut Self) -> T) -> T {
        let file = std::mem::replace(&mut self.file, file);
        let checked = check(self);
        self.file = file;
        checked
    }

    /// The source at `span` in the file being checked.
    fn written(&self, span: Span) -> &str {
        &self.sources[self.file][span.start..span.end]
    }

    fn this_schema(&self) -> String {
        "this schema".to_string()
    }

    pub(crate) fn schema(&mut self, files: Vec<SourceFile>, decoded: &'a [DecodedFile]) -> Schema {
        self.prelude_file = files.len() - 1;
        self.prelude_names = decoded[self.prelude_file]
            .document
            .definition
            .iter()
            .filter_map(|definition| match definition {
                ast::Definition::Record(d) => Some(record(d).body.name.value),
                ast::Definition::Choice(d) => Some(choice(d).body.name.value),
                ast::Definition::IntrinsicType(d) => Some(spanned_words(&d.name.name).value),
                ast::Definition::NamedType(d) => Some(spanned_words(&d.body.name).value),
                ast::Definition::UntaggedType(d) => Some(d.body.name.value.clone()),
                ast::Definition::TypeAlias(d) => Some(alias_shape(&d.body).name.value),
                _ => None,
            })
            .collect();
        self.paths = files.iter().map(|file| file.path.clone()).collect();
        self.sources = files.iter().map(|file| file.source.clone()).collect();
        self.imports = decoded.iter().map(|file| file.imports.clone()).collect();
        for (index, file) in decoded.iter().enumerate() {
            self.file = index;
            for definition in &file.document.definition {
                match definition {
                    ast::Definition::Record(found) => {
                        let found = record(found);
                        self.check_intrinsic(found.intrinsic);
                        self.declare_shaped_type(&found.body, found.public, "RECORD", "fields");
                    }
                    ast::Definition::Choice(found) => {
                        let found = choice(found);
                        let parameters: Vec<String> = found
                            .body
                            .parameters
                            .iter()
                            .map(|parameter| format!("{}: TYPE", parameter.name.value))
                            .collect();
                        let shown = fill_slots(&found.body.name.value, &parameters);
                        self.error(
                            found.body.name.span,
                            format!(
                                "found `CHOICE {shown} {{ … }}`; expected `TYPE ALIAS {shown}: UNION […]`, since a choice is now a union of its variants; write the variants in the brackets, as in `` TYPE ALIAS Case: UNION [`smart, `any] ``"
                            ),
                        );
                        // Read as the alias it should be, so that its uses
                        // aren't reported too.
                        self.declare_shaped_type(&found.body, found.public, "CHOICE", "variants");
                    }
                    ast::Definition::IntrinsicType(found) => {
                        self.check_intrinsic(Some(found.intrinsic));
                        let name = spanned_words(&found.name.name);
                        self.check_type_words(&name, false);
                        self.declare_type(&name, false);
                        self.declare_type_parameters(&[]);
                    }
                    ast::Definition::NamedType(found) => {
                        let name = spanned_words(&found.body.name);
                        // A slot's word may be lowercase, as `from` in
                        // `Range from (Integer) to (Integer)`.
                        self.check_type_words(&name, !found.body.arguments.is_empty());
                        self.declare_type(&name, found.public.is_some());
                        self.declare_type_parameters(&[]);
                    }
                    ast::Definition::UntaggedType(found) => {
                        let name = &found.body.name;
                        self.error(
                            name.span,
                            format!(
                                "found `TYPE {name}: …`; expected `TYPE ALIAS {name}: …`, the same type under another name, or `NAMED TYPE {name} (…)`, a type of its own whose values are written `{name} …`",
                                name = name.value
                            ),
                        );
                        self.check_type_words(name, false);
                        self.declare_type(name, found.public.is_some());
                        self.declare_type_parameters(&[]);
                    }
                    ast::Definition::TypeAlias(found) => match union_of_variants(found) {
                        Some(_) => self.declare_variants_alias(found),
                        None => self.declare_alias(found),
                    },
                    ast::Definition::Errors(found) => {
                        let found = errors(found);
                        self.check_intrinsic(found.intrinsic);
                        self.declare_group(found.body, found.public);
                    }
                    ast::Definition::Function(function) => self.error(
                        function.span,
                        "only the protocol prelude can declare a FUNCTION",
                    ),
                    ast::Definition::Method(_) | ast::Definition::Import(_) => {}
                }
            }
        }
        let mut types = Vec::new();
        let mut errors_found = Vec::new();
        let mut methods: Vec<Method> = Vec::new();
        for (index, file) in decoded.iter().enumerate() {
            self.file = index;
            for definition in &file.document.definition {
                match definition {
                    ast::Definition::Record(found) => {
                        let found = record(found);
                        let index = types.len();
                        let fields = self.generic(index, |checker| {
                            let fields = found.body.body.map_or(&[][..], |body| &body.value);
                            checker.fields(index, fields)
                        });
                        let kind = TypeKind::Record(fields);
                        if found.body.body.is_some() {
                            self.check_parameters_used(index, &kind);
                        }
                        types.push(TypeDefinition {
                            public: found.public || self.file == self.prelude_file,
                            doc: doc(found.doc),
                            name: found.body.name.value.clone(),
                            parameters: self.parameter_names(index),
                            kind,
                        });
                    }
                    ast::Definition::Choice(found) => {
                        let found = choice(found);
                        let index = types.len();
                        let variants = self.generic(index, |checker| {
                            let variants = found.body.body.map_or(&[][..], |body| &body.value);
                            checker.variants(&variants.iter().collect::<Vec<_>>())
                        });
                        let kind = TypeKind::Variants(variants);
                        if found.body.body.is_some() {
                            self.check_parameters_used(index, &kind);
                        }
                        types.push(TypeDefinition {
                            public: found.public || self.file == self.prelude_file,
                            doc: doc(found.doc),
                            name: found.body.name.value.clone(),
                            parameters: self.parameter_names(index),
                            kind,
                        });
                    }
                    // Reported when it was declared, and read as a named type
                    // so that its uses aren't reported too.
                    ast::Definition::IntrinsicType(found) => types.push(TypeDefinition {
                        public: false,
                        doc: doc(&found.doc),
                        name: spanned_words(&found.name.name).value,
                        parameters: Vec::new(),
                        kind: constructor_kind(ShapePart::of(&found.name.name.parts), Vec::new()),
                    }),
                    ast::Definition::NamedType(found) => {
                        let slots = found
                            .body
                            .arguments
                            .iter()
                            .map(|r#type| self.r#type(r#type))
                            .collect();
                        types.push(TypeDefinition {
                            public: found.public.is_some() || self.file == self.prelude_file,
                            doc: doc(&found.doc),
                            name: spanned_words(&found.body.name).value,
                            parameters: Vec::new(),
                            kind: constructor_kind(ShapePart::of(&found.body.name.parts), slots),
                        });
                    }
                    ast::Definition::UntaggedType(found) => {
                        let mut constructor: Vec<ShapePart> = found
                            .body
                            .name
                            .value
                            .split(' ')
                            .map(|word| ShapePart::Word(word.to_string()))
                            .collect();
                        constructor.push(ShapePart::Slot);
                        let slots = vec![self.r#type(&found.body.r#type)];
                        types.push(TypeDefinition {
                            public: found.public.is_some(),
                            doc: doc(&found.doc),
                            name: found.body.name.value.clone(),
                            parameters: Vec::new(),
                            kind: constructor_kind(constructor, slots),
                        });
                    }
                    ast::Definition::TypeAlias(found) => {
                        let Some(variants) = union_of_variants(found) else {
                            continue;
                        };
                        let index = types.len();
                        let variants = self.generic(index, |checker| checker.variants(&variants));
                        let kind = TypeKind::Variants(variants);
                        self.check_parameters_used(index, &kind);
                        types.push(TypeDefinition {
                            public: found.public.is_some() || self.file == self.prelude_file,
                            doc: doc(&found.doc),
                            name: alias_shape(&found.body).name.value,
                            parameters: self.parameter_names(index),
                            kind,
                        });
                    }
                    ast::Definition::Errors(_)
                    | ast::Definition::Method(_)
                    | ast::Definition::Function(_)
                    | ast::Definition::Import(_) => {}
                }
            }
        }
        for index in 0..self.aliases.len() {
            // Reports mistakes in an alias once, used or not.
            self.alias_type(index);
            self.check_alias_parameters_used(index);
        }
        for index in 0..self.groups.len() {
            let declared = &self.groups[index].declared;
            let (file, public) = (declared.file, declared.public);
            let own = self.groups[index].own;
            let own_errors = self.within(file, |checker| {
                checker.declare_errors(own, public, &mut errors_found)
            });
            self.groups[index].own_errors = own_errors;
        }
        for index in 0..self.groups.len() {
            // Reports unknown groups and cycles once, here.
            self.expand(index, &mut Vec::new());
        }
        for (index, file) in decoded.iter().enumerate() {
            self.file = index;
            for definition in &file.document.definition {
                if let ast::Definition::Method(found) = definition {
                    let method = self.method(found, &mut errors_found);
                    let parts = method_parts(found);
                    if let Some(undo) = parts.undone_by {
                        self.undos.push(PendingUndo {
                            method: methods.len(),
                            file: index,
                            undo,
                        });
                    }
                    if let Some(examples) = parts.examples {
                        self.examples.push(PendingExamples {
                            method: methods.len(),
                            file: index,
                            examples,
                        });
                    }
                    let overlapping = methods.iter().find(|other: &&Method| {
                        other.shape.text == method.shape.text
                            && other
                                .shape
                                .parameters
                                .iter()
                                .zip(&method.shape.parameters)
                                .all(|(a, b)| crate::annotation::overlaps(&a.r#type, &b.r#type))
                    });
                    if let Some(other) = overlapping {
                        let only_returns = match other
                            .returns
                            .as_ref()
                            .map(|returns| &returns.r#type)
                            != method.returns.as_ref().map(|returns| &returns.r#type)
                        {
                            true => {
                                "; overloads that differ only in what they return can't be told apart, since a call states only its arguments' types"
                            }
                            false => "",
                        };
                        let message = format!(
                            "found a second method `{}` with the same parameter types as one before it; expected the overloads of a shape in the protocol to differ in a parameter's type, so that a call's arguments choose one; rename one, or change a parameter's type{only_returns}",
                            method.shape.text
                        );
                        self.error(parts.shape.name.span, message);
                    }
                    methods.push(method);
                }
            }
        }
        let header = decoded
            .first()
            .and_then(|root| root.document.header.as_ref());
        let name = header.map(|header| {
            self.within(0, |checker| {
                let name = spanned_words(&header.name.name);
                checker.check_words(&name, "a protocol");
                name.value
            })
        });
        let mut schema = Schema {
            files,
            doc: header.and_then(|header| doc(&header.doc)),
            name: name.unwrap_or_default(),
            version: header.map_or(0, |header| header.version.value),
            built_ins: crate::schema::built_ins(&types),
            types,
            errors: errors_found,
            methods,
        };
        for pending in std::mem::take(&mut self.undos) {
            let undo = self.within(pending.file, |checker| {
                checker.undo(&schema, &schema.methods[pending.method], pending.undo)
            });
            schema.methods[pending.method].undone_by = undo;
        }
        self.check_recursion(&schema);
        self.check_variants_recursion(&schema);
        self.check_forms(&schema);
        self.check_unions(&schema);
        if self.diagnostics.is_empty() {
            // An alias makes no type in the schema, so its type is checked
            // here, used or not.
            let aliases: Vec<&Type> = self
                .aliases
                .iter()
                .filter_map(|alias| match &alias.resolved {
                    Resolved::Yes(Some(read)) => Some(read),
                    _ => None,
                })
                .collect();
            self.diagnostics
                .extend(crate::refinement::check_schema(&schema, &aliases));
            self.diagnostics
                .extend(crate::refinement::check_methods(&schema));
        }
        if self.diagnostics.is_empty() {
            self.read_defaults(&mut schema);
        }
        // Examples are read against the schema, defaults and all, which
        // must be whole first: a mistake elsewhere would only show up again
        // in them.
        if self.diagnostics.is_empty() {
            for pending in std::mem::take(&mut self.examples) {
                let examples = self.within(pending.file, |checker| {
                    checker.examples(&schema, &schema.methods[pending.method], pending.examples)
                });
                schema.methods[pending.method].examples = examples;
            }
        }
        schema
    }

    /// A field without a name is written as its variant alone, `` `by name ``,
    /// so its type's every value is a variant, and the variant's shape leads
    /// to one field: no two such fields of a record may share a variant, so
    /// a record value never has to try a field and back out.
    fn check_forms(&mut self, schema: &Schema) {
        for pending in std::mem::take(&mut self.forms) {
            let TypeKind::Record(fields) = &schema.types[pending.r#type].kind else {
                continue;
            };
            self.file = pending.file;
            let mut seen: Vec<(Written, usize)> = Vec::new();
            for (index, field) in fields.iter().enumerate() {
                if field.form != FieldForm::Unnamed
                    // A second field of the same name is reported already.
                    || fields[..index].iter().any(|other| other.name == field.name)
                {
                    continue;
                }
                let span = pending.spans[index];
                let forms = schema.written_forms(&field.r#type);
                let Some(forms) = forms
                    .filter(|forms| forms.iter().all(|form| matches!(form, Written::Variant(_))))
                else {
                    self.error(
                        span,
                        format!(
                            "found the field `{}` of {}; expected a union of variants, since the field is written as its variant alone; give the field a shape with words",
                            field.name,
                            code(&schema.type_name(&field.r#type))
                        ),
                    );
                    continue;
                };
                for form in forms {
                    let clash = seen
                        .iter()
                        .find(|(known, _)| known.admits(&form) || form.admits(known));
                    if let Some((known, other)) = clash {
                        self.error(
                            span,
                            format!(
                                "found {known} as a value of both fields `{}` and `{}`; expected each field written as its variant alone to have variants of its own, since the variant finds the field; give one of them a shape with words",
                                fields[*other].name, field.name
                            ),
                        );
                        break;
                    }
                    seen.push((form, index));
                }
            }
        }
    }

    /// A value's form tells its member of a union, in anyx and in JSON
    /// alike, so no two members of a union may share a form, whether
    /// written in it or made by the arguments of a generic type or alias:
    /// `Depth??` holds `null` in two ways.
    fn check_unions(&mut self, schema: &Schema) {
        let mut seen: Vec<(usize, Vec<Type>)> = Vec::new();
        for pending in std::mem::take(&mut self.unions) {
            self.file = pending.file;
            match pending.written {
                true => self.check_union(schema, &pending.r#type, pending.span),
                false => self.check_instance(
                    schema,
                    &pending.r#type,
                    &pending.parameters,
                    &pending.arguments,
                    pending.span,
                    &mut seen,
                ),
            }
        }
    }

    /// Checks the unions in `type` that its `parameters` take part in,
    /// with `arguments` in their place, and those of the generic types it
    /// uses. Unions without a parameter are checked where written.
    fn check_instance(
        &mut self,
        schema: &Schema,
        r#type: &Type,
        parameters: &[String],
        arguments: &[Type],
        span: Span,
        seen: &mut Vec<(usize, Vec<Type>)>,
    ) {
        match r#type {
            Type::Union(members) => {
                if !parameters.is_empty() && !r#type.parameters().is_empty() {
                    self.check_union(schema, &r#type.substitute(parameters, arguments), span);
                }
                for member in members {
                    self.check_instance(schema, member, parameters, arguments, span, seen);
                }
            }
            Type::List(item) | Type::Map(item) | Type::Refined(item, _) => {
                self.check_instance(schema, item, parameters, arguments, span, seen)
            }
            Type::Tuple(slots) => {
                for slot in slots {
                    self.check_instance(schema, slot, parameters, arguments, span, seen);
                }
            }
            Type::Variant(shape) => {
                for parameter in &shape.parameters {
                    self.check_instance(
                        schema,
                        &parameter.r#type,
                        parameters,
                        arguments,
                        span,
                        seen,
                    );
                }
            }
            Type::Generic(index, inner) => {
                let inner: Vec<Type> = inner
                    .iter()
                    .map(|argument| argument.substitute(parameters, arguments))
                    .collect();
                for argument in &inner {
                    self.check_instance(schema, argument, &[], &[], span, seen);
                }
                if seen
                    .iter()
                    .any(|(known, with)| known == index && *with == inner)
                {
                    return;
                }
                seen.push((*index, inner.clone()));
                let definition = &schema.types[*index];
                let held: Vec<&Type> = match &definition.kind {
                    TypeKind::Record(fields) => fields.iter().map(|field| &field.r#type).collect(),
                    TypeKind::Variants(variants) => variants
                        .iter()
                        .flat_map(|variant| &variant.shape.parameters)
                        .map(|parameter| &parameter.r#type)
                        .collect(),
                    TypeKind::Distinct {
                        underlying: underneath,
                        ..
                    } => vec![underneath],
                };
                for held in held {
                    self.check_instance(schema, held, &definition.parameters, &inner, span, seen);
                }
            }
            _ => {}
        }
    }

    /// Reports two members of the union `type` written alike, in anyx or
    /// in JSON. A repeat within one member is that member's to report.
    fn check_union(&mut self, schema: &Schema, r#type: &Type, span: Span) {
        let Type::Union(members) = r#type else {
            return;
        };
        let union = schema.type_name(r#type);
        let mut written: Vec<(Written, usize)> = Vec::new();
        let mut json: Vec<(crate::schema::JsonForm, usize)> = Vec::new();
        for (index, member) in members.iter().enumerate() {
            let clash = schema.written_forms(member).and_then(|forms| {
                forms.into_iter().find_map(|form| {
                    let clash = written
                        .iter()
                        .find(|(known, _)| known.admits(&form) || form.admits(known))
                        .map(|(known, other)| (known.to_string(), *other));
                    written.push((form, index));
                    clash
                })
            });
            let clash = clash.or_else(|| {
                schema.json_forms(member).and_then(|forms| {
                    forms.into_iter().find_map(|form| {
                        let clash = json
                            .iter()
                            .find(|(known, _)| *known == form)
                            .map(|(known, other)| (format!("{known} in JSON"), *other));
                        json.push((form, index));
                        clash
                    })
                })
            });
            if let Some((form, other)) = clash {
                self.error(
                    span,
                    format!(
                        "found {form} as a value of both {} and {} in {}; expected each member of a union written in forms of its own, since a value's form alone tells its member; leave one of them out, or give it a shape of its own",
                        code(&schema.type_name(&members[other])),
                        code(&schema.type_name(member)),
                        code(&union),
                    ),
                );
            }
        }
    }

    /// Reads each field's `DEFAULT` as an entry of the field. A
    /// default may be a record whose own fields have defaults, so it is
    /// read again until every default it holds is filled in.
    fn read_defaults(&mut self, schema: &mut Schema) {
        let pending = std::mem::take(&mut self.defaults);
        for _ in 0..=pending.len() {
            let mut changed = false;
            for default in &pending {
                let TypeKind::Record(fields) = &schema.types[default.r#type].kind else {
                    continue;
                };
                let field = &fields[default.field];
                let read =
                    crate::codec::decode_entry(schema, fields, default.field, &default.entry);
                let name = field.name.clone();
                match read {
                    Ok(value) => {
                        let TypeKind::Record(fields) = &mut schema.types[default.r#type].kind
                        else {
                            continue;
                        };
                        let slot = &mut fields[default.field].default;
                        if slot.as_ref() != Some(&value) {
                            *slot = Some(value);
                            changed = true;
                        }
                    }
                    Err(mismatch) => {
                        self.within(default.file, |checker| {
                            checker.error(
                                mismatch.span,
                                format!("in the `DEFAULT` of `{name}`: {}", mismatch.message),
                            )
                        });
                    }
                }
            }
            if !changed || !self.diagnostics.is_empty() {
                break;
            }
        }
    }

    /// Reads each example of `method`: its call must be a call of `method`,
    /// and its outcome one that `method` may have.
    fn examples(
        &mut self,
        schema: &Schema,
        method: &Method,
        examples: &ast::Examples,
    ) -> Vec<Example> {
        if examples.example.is_empty() {
            self.error(
                examples.span,
                format!(
                    "found `EXAMPLES` without an example; expected a call of `{}` and its outcome in the brackets, such as `{} RETURNS …`, or no `EXAMPLES` at all",
                    method.full_name(),
                    method.full_name()
                ),
            );
        }
        examples
            .example
            .iter()
            .filter_map(|example| {
                let (call, written, span) = match example {
                    ast::Example::ReturnsNothing { call, span } => {
                        (call, WrittenOutcome::ReturnsNothing, *span)
                    }
                    ast::Example::Returns { call, value, span } => {
                        (call, WrittenOutcome::Returns(value), *span)
                    }
                    ast::Example::Error { call, error, span } => {
                        (call, WrittenOutcome::Error(error), *span)
                    }
                };
                let (doc, call) = Self::without_doc(call);
                let read = codec::decode_call_of(schema, &self.sources[self.file], method, &call)
                    .and_then(|arguments| {
                        let outcome = response::read_outcome(schema, method, written, span)?;
                        crate::refinement::check_postcondition(
                            schema, method, &arguments, &outcome,
                        )
                        .map_err(|message| codec::Mismatch { span, message })?;
                        Ok((arguments, outcome))
                    });
                match read {
                    Ok((arguments, outcome)) => Some(Example {
                        doc,
                        text: self
                            .written(Span::new(call.span.start, span.end))
                            .to_string(),
                        call: self.written(call.span).to_string(),
                        arguments,
                        outcome,
                    }),
                    Err(mismatch) => {
                        self.error(
                            mismatch.span,
                            format!(
                                "in an example of `{}`: {}",
                                method.full_name(),
                                mismatch.message
                            ),
                        );
                        None
                    }
                }
            })
            .collect()
    }

    /// `node` without the `#` doc written before it, which lands in the
    /// first slot of its leftmost call, and the doc: in `# Saving.` then
    /// `files/write "a"`, the doc is the first argument of the call
    /// `_ files`.
    fn without_doc(node: &Node) -> (Option<String>, Node) {
        let unchanged = || (None, node.clone());
        let NodeKind::Call(call) = &node.kind else {
            return unchanged();
        };
        let (Some(NamePart::Slot), Some(first)) = (call.name.first(), call.arguments.first())
        else {
            return unchanged();
        };
        let (doc, first) = match &first.kind {
            NodeKind::String(literal)
                if literal.kind == StringKind::Line && call.name.len() > 1 =>
            {
                (literal.value.clone(), None)
            }
            NodeKind::Call(_) => match Self::without_doc(first) {
                (Some(doc), first) => (doc, Some(first)),
                (None, _) => return unchanged(),
            },
            _ => return unchanged(),
        };
        let mut call = call.clone();
        match first {
            Some(first) => call.arguments[0] = first,
            None => {
                call.name.remove(0);
                call.arguments.remove(0);
            }
        }
        let start = call
            .name
            .iter()
            .filter_map(|part| match part {
                NamePart::Word { span, .. } => Some(span.start),
                NamePart::Slot => None,
            })
            .chain(call.arguments.iter().map(|argument| argument.span.start))
            .min()
            .unwrap_or(node.span.start);
        (
            Some(doc),
            Node {
                kind: NodeKind::Call(call),
                span: Span::new(start, node.span.end),
            },
        )
    }

    /// Declares a `RECORD` or `CHOICE` by its shape, after checking that
    /// the shape has words and its braces come last.
    fn declare_shaped_type<T>(
        &mut self,
        shape: &TypeShape<'_, T>,
        public: bool,
        keyword: &str,
        what: &str,
    ) {
        if shape.words.value.is_empty() {
            self.error(
                shape.name.span,
                format!(
                    "found a `{keyword}` whose name has no words; expected at least one word to name it, with any type parameters among the words, as in `{keyword} Page of (Item: TYPE)`"
                ),
            );
        } else {
            self.check_type_words(&shape.words, !shape.parameters.is_empty());
        }
        let parameters: Vec<String> = shape
            .parameters
            .iter()
            .map(|parameter| parameter.name.value.clone())
            .collect();
        if shape.body.is_none() && shape.misplaced.is_empty() {
            let declared: Vec<String> = parameters
                .iter()
                .map(|parameter| format!("{parameter}: TYPE"))
                .collect();
            self.error(
                shape.name.span,
                format!(
                    "found the `{keyword}` `{}` without its {what} in braces; expected them last, after its name, as in `{keyword} {} {{ … }}`",
                    fill_slots(&shape.name.value, &parameters),
                    fill_slots(&shape.name.value, &declared),
                ),
            );
        }
        for span in &shape.misplaced {
            self.error(
                *span,
                format!(
                    "found braces in the name of a `{keyword}`, before its end; expected its {what} in one pair of braces, last, and only type parameters such as `(Item: TYPE)` in the name's other slots"
                ),
            );
        }
        self.declare_type(&shape.name, public);
        self.declare_type_parameters(&shape.parameters);
    }

    /// Declares a type by its shape, `page of _`, and reports a second type
    /// of that shape in the protocol.
    fn declare_type(&mut self, name: &ast::Spanned<String>, public: bool) {
        if self.file != self.prelude_file
            && (crate::prelude::find(&name.value).is_some()
                || self.prelude_names.contains(&name.value))
        {
            self.error(
                name.span,
                format!(
                    "found a type named `{}`, which the prelude already declares as a built-in type; expected a name of its own, since the prelude's types are visible in every file; rename it",
                    name.value
                ),
            );
        }
        // A user shadowing the prelude was diagnosed at the user declaration.
        // Still check duplicates within the prelude itself.
        let user_shadow = self.file == self.prelude_file
            && self
                .types
                .iter()
                .any(|known| known.file != self.prelude_file && known.name == name.value);
        if !user_shadow {
            self.check_duplicate_name(name);
        }
        let declared = self.declared(name, public);
        self.types.push(declared);
    }

    /// The type at `index` with its parameters in its slots, as a message
    /// shows it: `page of (Item)`.
    fn type_shown(&self, index: usize) -> String {
        fill_slots(&self.types[index].name, &self.parameter_names(index))
    }

    /// Declares the type parameters in the slots of the type just declared,
    /// each a name once.
    fn declare_type_parameters(&mut self, written: &[&ast::TypeParameter]) {
        let names = self.type_parameter_names(written);
        self.type_parameters.push(names);
    }

    /// The names of type parameters, each in Title Case and once.
    fn type_parameter_names(
        &mut self,
        written: &[&ast::TypeParameter],
    ) -> Vec<ast::Spanned<String>> {
        let mut names: Vec<ast::Spanned<String>> = Vec::new();
        for parameter in written {
            self.check_type_parameter_words(&parameter.name);
            if names
                .iter()
                .any(|known| known.value == parameter.name.value)
            {
                self.error(
                    parameter.name.span,
                    format!(
                        "found a second type parameter named `{}`; expected each type parameter once; rename one of them",
                        parameter.name.value
                    ),
                );
                continue;
            }
            names.push(parameter.name.clone());
        }
        names
    }

    /// A type parameter stands for a type, so its name is Title Case, as a
    /// type's is: `Item`, `Sort Key`.
    fn check_type_parameter_words(&mut self, name: &ast::Spanned<String>) {
        let fits = name.value.split(' ').all(|word| {
            word.starts_with(|c: char| c.is_ascii_uppercase())
                && word
                    .bytes()
                    .skip(1)
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        });
        if !fits {
            self.error(
                name.span,
                format!(
                    "found `{}` as the name of a type parameter; expected words in Title Case, such as `Item`, since a type parameter stands for a type; rename it",
                    name.value
                ),
            );
        }
    }

    /// Checks an alias's name: words to name it, the type it stands for
    /// last, and only type parameters in its other slots.
    fn check_alias_shape(&mut self, alias: &ast::TypeAlias, shape: &AliasShape<'_>) {
        if shape.words.value.is_empty() {
            self.error(
                shape.name.span,
                "found a `TYPE ALIAS` whose name has no words; expected at least one word to name it, as in `TYPE ALIAS Paths: LIST Text`",
            );
        } else {
            let slots = !shape.parameters.is_empty() || !shape.misplaced.is_empty();
            self.check_type_words(&shape.words, slots);
        }
        if shape.r#type.is_none() {
            self.error(
                alias.body.span,
                format!(
                    "found the `TYPE ALIAS` `{}` without the type it stands for; expected it last, after a colon, as in `TYPE ALIAS {}: LIST Text`",
                    shape.name.value, shape.name.value
                ),
            );
        }
        for span in &shape.misplaced {
            self.error(
                *span,
                "found a type in the name of a `TYPE ALIAS`, before its end; expected only type parameters such as `(Item: TYPE)` there, and the type it stands for last",
            );
        }
    }

    /// Declares an alias of a union of variants as a type, so that it
    /// keeps its name: `` TYPE ALIAS Case: UNION [`smart, `any] ``.
    fn declare_variants_alias(&mut self, alias: &ast::TypeAlias) {
        let shape = alias_shape(&alias.body);
        self.check_alias_shape(alias, &shape);
        self.declare_type(&shape.name, alias.public.is_some());
        self.declare_type_parameters(&shape.parameters);
    }

    /// Declares a `TYPE ALIAS` by its shape, after checking that the type
    /// it stands for fills the last slot and type parameters the others.
    fn declare_alias(&mut self, alias: &'a ast::TypeAlias) {
        let shape = alias_shape(&alias.body);
        self.check_alias_shape(alias, &shape);
        if self.file != self.prelude_file
            && (crate::prelude::find(&shape.name.value).is_some()
                || self.prelude_names.contains(&shape.name.value))
        {
            self.error(
                shape.name.span,
                format!(
                    "found a type named `{}`, which the prelude already declares as a built-in type; expected a name of its own, since the prelude's types are visible in every file; rename it",
                    shape.name.value
                ),
            );
        }
        let user_shadow = self.file == self.prelude_file
            && self.aliases.iter().any(|known| {
                known.declared.file != self.prelude_file && known.declared.name == shape.name.value
            });
        if !user_shadow {
            self.check_duplicate_name(&shape.name);
        }
        let declared = self.declared(&shape.name, alias.public.is_some());
        let parameters = self.type_parameter_names(&shape.parameters);
        self.aliases.push(Alias {
            declared,
            parameters,
            written: shape.r#type,
            resolved: Resolved::No,
        });
    }

    /// The type the alias at `index` stands for, with its type parameters
    /// in it, read once. `None` when it leads back to the alias, or is
    /// missing; both are reported once.
    fn alias_type(&mut self, index: usize) -> Option<Type> {
        match &self.aliases[index].resolved {
            Resolved::Yes(r#type) => return r#type.clone(),
            Resolved::Reading => {
                let declared = &self.aliases[index].declared;
                let (file, span, name) = (declared.file, declared.span, declared.name.clone());
                self.within(file, |checker| {
                    checker.error(
                        span,
                        format!(
                            "found the type alias `{name}` standing for a type that leads back to it; expected an alias of a type that doesn't, since an alias is only another name for its type; to hold values of its own kind, use a `RECORD` or a `NAMED TYPE` that holds them through a `LIST`"
                        ),
                    )
                });
                self.aliases[index].resolved = Resolved::Yes(None);
                return None;
            }
            Resolved::No => {}
        }
        self.aliases[index].resolved = Resolved::Reading;
        let alias = &self.aliases[index];
        let (file, written) = (alias.declared.file, alias.written);
        let names: Vec<String> = alias
            .parameters
            .iter()
            .map(|parameter| parameter.value.clone())
            .collect();
        let shadowing = alias.parameters.clone();
        let owner = format!("the type alias `{}`", alias_shown(alias));
        let read = self.within(file, |checker| {
            for parameter in &shadowing {
                checker.check_parameter_shadows(parameter, &owner);
            }
            let outer = std::mem::replace(&mut checker.parameters, names);
            let read = written.map(|written| checker.r#type(written));
            checker.parameters = outer;
            read
        });
        // A use within its own type has already reported the cycle.
        if let Resolved::Reading = self.aliases[index].resolved {
            self.aliases[index].resolved = Resolved::Yes(read.clone());
            return read;
        }
        None
    }

    /// Reports each type parameter of the alias at `index` that the type
    /// it stands for doesn't use.
    fn check_alias_parameters_used(&mut self, index: usize) {
        let Resolved::Yes(Some(r#type)) = &self.aliases[index].resolved else {
            return;
        };
        let used: Vec<String> = r#type
            .parameters()
            .into_iter()
            .map(str::to_string)
            .collect();
        let alias = &self.aliases[index];
        let (file, shown) = (alias.declared.file, alias_shown(alias));
        let unused: Vec<ast::Spanned<String>> = alias
            .parameters
            .iter()
            .filter(|parameter| !used.contains(&parameter.value))
            .cloned()
            .collect();
        self.within(file, |checker| {
            for parameter in unused {
                checker.error(
                    parameter.span,
                    format!(
                        "found the type parameter `{parameter}` of the type alias `{shown}`, which the type it stands for doesn't use; expected each parameter in that type, such as `LIST {parameter}`; use it or remove it",
                        parameter = parameter.value,
                    ),
                );
            }
        });
    }

    /// Reports a type parameter with the name of a type visible here, since
    /// its name would then mean either.
    fn check_parameter_shadows(&mut self, parameter: &ast::Spanned<String>, owner: &str) {
        let shadowed: Vec<String> = self
            .types
            .iter()
            .chain(self.aliases.iter().map(|alias| &alias.declared))
            .filter(|known| known.name == parameter.value && self.accessible(known))
            .map(|known| format!("`{}`", known.name))
            .collect();
        if !shadowed.is_empty() {
            self.error(
                parameter.span,
                format!(
                    "found the type parameter `{name}` of {owner}, which has the name of the type {}; expected a name that no type visible here has, since `{name}` there would then mean either; rename the parameter",
                    shadowed.join(", "),
                    name = parameter.value,
                ),
            );
        }
    }

    fn parameter_names(&self, index: usize) -> Vec<String> {
        self.type_parameters[index]
            .iter()
            .map(|name| name.value.clone())
            .collect()
    }

    /// Checks the fields or variants of the type at `index` with its type
    /// parameters in scope, after checking that no type here has the name
    /// of one of them.
    fn generic<T>(&mut self, index: usize, check: impl FnOnce(&mut Self) -> T) -> T {
        let type_name = self.type_shown(index);
        for parameter in self.type_parameters[index].clone() {
            let shadowed: Vec<String> = self
                .types
                .iter()
                .filter(|known| known.name == parameter.value && self.accessible(known))
                .map(|known| format!("`{}`", known.name.clone()))
                .collect();
            if !shadowed.is_empty() {
                self.error(
                    parameter.span,
                    format!(
                        "found the type parameter `{name}` of `{type_name}`, which has the name of the type {}; expected a name that no type visible here has, since `{name}` in `{type_name}` would then mean either; rename the parameter",
                        shadowed.join(", "),
                        name = parameter.value,
                    ),
                );
            }
        }
        let names = self.parameter_names(index);
        let parameters = std::mem::replace(&mut self.parameters, names);
        let checked = check(self);
        self.parameters = parameters;
        checked
    }

    /// Reports each type parameter of the type at `index` that none of its
    /// fields or variants uses.
    fn check_parameters_used(&mut self, index: usize, kind: &TypeKind) {
        let types: Vec<&Type> = match kind {
            TypeKind::Record(fields) => fields.iter().map(|field| &field.r#type).collect(),
            TypeKind::Variants(variants) => variants
                .iter()
                .flat_map(|variant| &variant.shape.parameters)
                .map(|parameter| &parameter.r#type)
                .collect(),
            TypeKind::Distinct { underlying, .. } => vec![underlying],
        };
        let used: Vec<&str> = types
            .iter()
            .flat_map(|r#type| r#type.parameters())
            .collect();
        let type_name = self.type_shown(index);
        for parameter in self.type_parameters[index].clone() {
            if !used.contains(&parameter.value.as_str()) {
                self.error(
                    parameter.span,
                    format!(
                        "found the type parameter `{name}` of `{type_name}`, which none of its types uses; expected each parameter in the type of a field or a variant's slot, such as `LIST: {name}`; use it or remove it",
                        name = parameter.value,
                    ),
                );
            }
        }
    }

    fn declared(&self, name: &ast::Spanned<String>, public: bool) -> Declared {
        Declared {
            name: name.value.clone(),
            span: name.span,
            file: self.file,
            public,
        }
    }

    /// Declares the group an `ERRORS` defines, which starts with its name
    /// and its own errors.
    fn declare_group(&mut self, list: &'a ast::ErrorList, public: bool) {
        let Some(terms) = self.error_terms(list) else {
            return;
        };
        let Some(name) = terms.name else {
            self.error(
                error_terms::span(list),
                "found an `ERRORS` without a name before its errors; expected a group's name, its own errors in brackets, then any groups it joins with `+`, such as `` ERRORS file errors [`trashed, `missing] ``",
            );
            return;
        };
        self.check_words(&name, "an error group");
        self.check_duplicate_name(&name);
        let declared = self.declared(&name, public);
        self.groups.push(Group {
            declared,
            own: terms.own,
            own_errors: Vec::new(),
            joined: terms.groups,
            expanded: None,
        });
    }

    /// `list` read as its terms, or `None` after reporting what doesn't fit.
    fn error_terms(&mut self, list: &'a ast::ErrorList) -> Option<ErrorTerms<'a>> {
        match error_terms::read(list) {
            Ok(terms) => Some(terms),
            Err(Misplaced::NotATerm(span)) => {
                let written = self.written(span).to_string();
                self.error(
                    span,
                    format!(
                        "found `{written}` among errors; expected errors in brackets, then groups joined with `+`, such as `[missing (path: Text)] + file errors` or `[missing] + files/file errors`"
                    ),
                );
                None
            }
            Err(Misplaced::OwnAfterGroup(span)) => {
                self.error(
                    span,
                    "found errors in brackets after a `+`; expected the errors in brackets first, then the groups joined with `+`, such as `[missing] + file errors`",
                );
                None
            }
        }
    }

    /// Reads the errors defined in brackets, as indices into `found`, each
    /// once in the protocol.
    fn declare_errors(
        &mut self,
        own: &[ast::Error],
        public: bool,
        found: &mut Vec<ErrorDefinition>,
    ) -> Vec<usize> {
        own.iter()
            .map(|error| {
                let shaped = self.shaped(error, "an error");
                let words = shaped.shape.words();
                if let Some(other) = self
                    .errors
                    .iter()
                    .find(|known| known.name == words)
                {
                    let line = anyx::line_column(&self.sources[other.file], other.span.start).line;
                    self.error(
                        error.body.name.span,
                        format!(
                            "found a second error `{words}` in {}, after the one on line {line}; expected each error once in the protocol; to share one, put it in an `ERRORS` group and join the group with `+`",
                            self.this_schema()
                        ),
                    );
                }
                let declared = self.declared(
                    &ast::Spanned {
                        value: words,
                        span: error.body.name.span,
                    },
                    public,
                );
                self.errors.push(declared);
                found.push(ErrorDefinition {

                    public,
                    error: shaped,
                });
                found.len() - 1
            })
            .collect()
    }

    /// A plain type shape, with its type arguments. Rejects legacy file paths.
    fn path<'r>(&mut self, reference: &'r ast::Reference) -> Option<(Path, &'r [ast::Type])> {
        let parts = &reference.name.parts;
        let slashed = parts
            .iter()
            .any(|part| matches!(part, NamePart::Word { text, .. } if text == "/"));
        if !slashed {
            return Some((
                Path {
                    name: reference.name.text.clone(),
                },
                &reference.arguments,
            ));
        }
        self.error(reference.span, "expected a plain type name or generic shape; import its file with `USE` instead of writing a path");
        None
    }

    fn check_duplicate_name(&mut self, name: &ast::Spanned<String>) {
        if let Some(known) = self
            .types
            .iter()
            .chain(self.groups.iter().map(|group| &group.declared))
            .chain(self.aliases.iter().map(|alias| &alias.declared))
            .find(|known| known.name == name.value)
        {
            self.error(name.span, format!("duplicate declaration `{}` in `{}` and `{}`; every type and error group must have a unique name in the protocol", name.value, self.paths[known.file], self.paths[self.file]));
        }
    }

    fn accessible(&self, declared: &Declared) -> bool {
        if declared.file == self.prelude_file {
            return true;
        }
        declared.file == self.file
            || (declared.public && self.imports[self.file].contains(&declared.file))
    }

    fn unavailable(&mut self, span: Span, name: &str, file: usize, public: bool) {
        let reason = if public {
            "add a direct `USE` of that file; imports are not re-exported"
        } else {
            "mark it `PUBLIC` and add a direct `USE` of that file"
        };
        self.error(
            span,
            format!(
                "`{name}` is declared in `{}` but is not visible in `{}`; {reason}",
                self.paths[file], self.paths[self.file]
            ),
        );
    }

    /// The type `path` names, as an index into the types. `span` is where
    /// it is written, and `arguments` how many types its slots hold.
    fn resolve_type(&mut self, span: Span, path: &Path, arguments: usize) -> Option<usize> {
        let found = self.types.iter().position(|known| known.name == path.name);
        if let Some(index) = found {
            let known = &self.types[index];
            if !self.accessible(known) {
                self.unavailable(span, &path.name, known.file, known.public);
            }
            return Some(index);
        }
        let written = self.written(span).to_string();
        let scope = self.this_schema();
        let words = |name: &str| {
            name.split(' ')
                .filter(|word| *word != "_")
                .collect::<Vec<_>>()
                .join(" ")
        };
        let wanted = words(&path.name);
        let shown = |checker: &Self, indices: &[usize]| {
            indices
                .iter()
                .map(|index| format!("`{}`", checker.type_shown(*index)))
                .collect::<Vec<_>>()
                .join(", ")
        };
        let in_scope = |known: &Declared| self.accessible(known);
        let same_words: Vec<usize> = (0..self.types.len())
            .filter(|index| {
                in_scope(&self.types[*index]) && words(&self.types[*index].name) == wanted
            })
            .collect();
        let aliases_with_words: Vec<String> = self
            .aliases
            .iter()
            .filter(|alias| in_scope(&alias.declared) && words(&alias.declared.name) == wanted)
            .map(|alias| format!("`{}`", alias_shown(alias)))
            .collect();
        if !same_words.is_empty() || !aliases_with_words.is_empty() {
            let count = match arguments {
                0 => "no type in a slot".to_string(),
                1 => "a type in 1 slot".to_string(),
                count => format!("types in {count} slots"),
            };
            self.error(
                span,
                format!(
                    "found `{written}`, with {count}, but {scope} has no type of that shape; the types with its words are {}; expected one of them, with a type in parentheses in each slot it has",
                    [shown(self, &same_words)]
                        .into_iter()
                        .filter(|shown| !shown.is_empty())
                        .chain(aliases_with_words)
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            );
            return None;
        }
        let first_word = wanted.split(' ').next().unwrap_or_default().to_string();
        let similar: Vec<usize> = (0..self.types.len())
            .filter(|index| {
                in_scope(&self.types[*index])
                    && words(&self.types[*index].name).split(' ').next() == Some(&first_word)
            })
            .collect();
        let similar = match similar.is_empty() {
            true => String::new(),
            false => format!(
                "; the types of {scope} that start with `{first_word}` are {}",
                shown(self, &similar)
            ),
        };
        let parameter = match self.parameters.contains(&first_word) {
            true => format!(
                "; `{first_word}` is a type parameter, which stands for a whole type and has no slots"
            ),
            false => String::new(),
        };
        let similar = similar + &parameter;
        let message = format!(
            "found `{written}`, which is not a type of {scope}; expected a built-in type ({built_in}), `LIST: type`, `MAP: type`, or a `RECORD`, `NAMED TYPE` or `TYPE ALIAS` defined here; define `{written}`, or fix the name; import the declaration directly with `USE` if it is in another file{similar}",
            built_in = crate::prelude::intrinsics()
                .iter()
                .map(|intrinsic| format!("`{}`", intrinsic.name))
                .collect::<Vec<_>>()
                .join(", "),
        );
        self.error(span, message);
        None
    }

    fn resolve_group(&mut self, reference: &GroupReference) -> Option<usize> {
        match self
            .groups
            .iter()
            .position(|group| group.declared.name == reference.name)
        {
            Some(index) => {
                let known = &self.groups[index].declared;
                if !self.accessible(known) {
                    self.unavailable(reference.span, &reference.name, known.file, known.public);
                }
                Some(index)
            }
            None => {
                self.error(reference.span, format!("found `{}`, which is not a visible error group; expected the name of an `ERRORS` group; define it or import its file with `USE`", reference.name));
                None
            }
        }
    }

    /// The errors that the group at `index` stands for, in order: its own,
    /// then those of the groups it joins. `None` when one of those can't be
    /// resolved. `visiting` holds the groups being expanded, to catch one
    /// that joins itself.
    fn expand(&mut self, index: usize, visiting: &mut Vec<usize>) -> Option<Vec<usize>> {
        if let Some(expanded) = &self.groups[index].expanded {
            return expanded.clone();
        }
        let group = &self.groups[index];
        let (file, span, name) = (
            group.declared.file,
            group.declared.span,
            group.declared.name.clone(),
        );
        let (own, joined) = (group.own_errors.clone(), group.joined.clone());
        if visiting.contains(&index) {
            self.within(file, |checker| {
                checker.error(
                    span,
                    format!(
                        "found the error group `{name}` joining itself; expected a group to join only groups that don't lead back to it"
                    ),
                )
            });
            return None;
        }
        visiting.push(index);
        let expanded = self.within(file, |checker| {
            joined.iter().try_fold(own, |mut expanded, reference| {
                if let Some(found) = checker.resolve_group(reference) {
                    expanded.extend(checker.expand(found, visiting)?);
                }
                Some(expanded)
            })
        });
        visiting.pop();
        self.groups[index].expanded = Some(expanded.clone());
        expanded
    }

    /// `INTRINSIC` before a declaration of a schema: only the prelude may
    /// write it.
    fn check_intrinsic(&mut self, intrinsic: Option<Span>) {
        if let Some(span) = intrinsic {
            self.error(
                span,
                "found `INTRINSIC` in a schema; expected it only in the prelude, which declares the built-in types, since the implementation supplies what an `INTRINSIC` declaration means; remove it",
            );
        }
    }

    /// Simple type names use Title Case. Generic shapes can use connective
    /// words or the nullable `?` operator around their type parameters.
    fn check_type_words(&mut self, name: &ast::Spanned<String>, generic: bool) {
        let fits = name.value.split(' ').all(|word| {
            if generic {
                return word == "?"
                    || word.starts_with(|c: char| c.is_ascii_alphabetic())
                        && word.bytes().all(|c| c.is_ascii_alphanumeric());
            }
            word.starts_with(|c: char| c.is_ascii_uppercase())
                && word
                    .bytes()
                    .skip(1)
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        });
        if !fits {
            let expected = if generic {
                "words of ASCII letters and digits, each starting with a letter, such as `Page of`"
            } else {
                "words in Title Case, such as `File Summary`"
            };
            self.error(
                name.span,
                format!(
                    "found `{}` as the name of a type; expected {expected}; rename it",
                    name.value
                ),
            );
        }
    }

    /// Names become Rust and TypeScript identifiers, so each word is
    /// lowercase letters and digits.
    fn check_words(&mut self, name: &ast::Spanned<String>, what: &str) {
        let fits = name.value.split(' ').all(|word| {
            word.starts_with(|c: char| c.is_ascii_lowercase())
                && word
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        });
        if !fits {
            self.error(
                name.span,
                format!(
                    "found `{}` as the name of {what}; expected words of lowercase ASCII letters and digits, each starting with a letter, since names become Rust and TypeScript identifiers; rename it",
                    name.value
                ),
            );
        }
    }

    fn fields(&mut self, record: usize, fields: &[ast::Field]) -> Vec<Field> {
        let mut checked: Vec<Field> = Vec::new();
        let mut spans = Vec::new();
        for field in fields {
            let (field, span) = self.field(record, checked.len(), field);
            if checked.iter().any(|known| known.name == field.name) {
                self.error(
                    span,
                    format!(
                        "found a second field named `{}`; expected each field once; rename or remove one of them",
                        field.name
                    ),
                );
            }
            checked.push(field);
            spans.push(span);
        }
        self.forms.push(PendingForms {
            r#type: record,
            file: self.file,
            spans,
        });
        checked
    }

    /// A field, and the span of its name. Every field is a shape: words
    /// alone are a checkbox, `` `(order: Order) `` a field written as its
    /// variant alone, and any other shape holds the field's type in its one slot, or
    /// a tuple of its slots' types when it has several. A `DEFAULT` holds
    /// an entry of the field, read once every type is known.
    fn field(&mut self, record: usize, index: usize, field: &ast::Field) -> (Field, Span) {
        let (declared, default) = match field {
            ast::Field::Defaulted { field, default, .. } => (field, Some(default)),
            ast::Field::Declared(field) => (field, None),
        };
        let (field, span) = self.declared_field(declared);
        if let Some(default) = default {
            match field.form {
                // A checkbox left out is `false`.
                FieldForm::Checkbox => self.error(
                    default.span,
                    format!(
                        "found a `DEFAULT` on the checkbox `{}`; expected none, since a checkbox left out is `false`",
                        field.name
                    ),
                ),
                _ => self.defaults.push(PendingDefault {
                    r#type: record,
                    field: index,
                    entry: default.clone(),
                    file: self.file,
                }),
            }
        }
        (field, span)
    }

    fn declared_field(&mut self, field: &ast::DeclaredField) -> (Field, Span) {
        let (name, arguments) = match field {
            ast::DeclaredField::Unnamed { doc, parameter, .. } => {
                self.check_words(&parameter.name, "a field");
                let field = Field {
                    doc: self::doc(doc),
                    name: parameter.name.value.clone(),
                    r#type: self.r#type(&parameter.r#type),
                    default: None,
                    form: FieldForm::Unnamed,
                };
                return (field, parameter.name.span);
            }
            ast::DeclaredField::Words {
                name, arguments, ..
            } => (name, arguments),
        };
        let mut slots = arguments.iter().peekable();
        // A doc fills the call's first slot, before the first word.
        let doc = match (name.parts.first(), slots.peek()) {
            (Some(NamePart::Slot), Some(ast::FieldSlot::Doc(doc))) => {
                slots.next();
                Some(doc.value.value.clone())
            }
            _ => None,
        };
        let slots: Vec<&ast::FieldSlot> = slots.collect();
        let mut name = name.clone();
        if doc.is_some() {
            name.parts.remove(0);
            name.text = name.text.trim_start_matches('_').trim_start().to_string();
        }
        let words = spanned_words(&name);
        self.check_words(&words, "a field");
        if slots.is_empty() {
            let boolean = self
                .types
                .iter()
                .position(|known| known.name == "Boolean" && known.file == self.prelude_file)
                .map_or(Type::Text, Type::Named);
            let field = Field {
                doc,
                name: words.value,
                r#type: boolean,
                default: Some(crate::value::Value::Variant {
                    shape: "false".into(),
                    arguments: Vec::new(),
                }),
                form: FieldForm::Checkbox,
            };
            return (field, name.span);
        }
        let types: Vec<Type> = slots
            .iter()
            .map(|slot| match slot {
                ast::FieldSlot::Type(r#type) => self.r#type(r#type),
                ast::FieldSlot::Doc(doc) => {
                    self.error(
                        doc.span,
                        "found a string in a field's slot; expected the field's type, as in `at least (Size) bytes`; a field's `#` doc goes on the lines before it",
                    );
                    Type::Text
                }
            })
            .collect();
        let r#type = match <[Type; 1]>::try_from(types) {
            Ok([r#type]) => r#type,
            Err(types) => Type::Tuple(types),
        };
        let field = Field {
            doc,
            name: words.value,
            r#type,
            default: None,
            form: FieldForm::Shape(name.text.clone()),
        };
        (field, name.span)
    }

    fn variants(&mut self, variants: &[&ast::Variant]) -> Vec<Shaped> {
        let mut checked: Vec<Shaped> = Vec::new();
        for variant in variants {
            let shaped = self.shaped(variant, "a variant");
            if checked
                .iter()
                .any(|known| known.shape.words() == shaped.shape.words())
            {
                self.error(
                    variant.body.name.span,
                    format!(
                        "found a second variant named `{}`; expected each variant's words once; rename or remove one of them",
                        shaped.shape.words()
                    ),
                );
            }
            checked.push(shaped);
        }
        checked
    }

    /// A variant or an error: its doc, then its shape after the backtick.
    fn shaped(&mut self, variant: &ast::Variant, what: &str) -> Shaped {
        let doc = variant.doc.as_ref().map(|doc| doc.value.value.clone());
        let shape = self.shape(&variant.body.name, &variant.body.arguments, what);
        Shaped { doc, shape }
    }

    fn shape(&mut self, name: &ast::CallName, parameters: &[ast::Parameter], what: &str) -> Shape {
        let words = spanned_words(name);
        if words.value.is_empty() {
            self.error(
                name.span,
                format!(
                    "found {what} with no words; expected at least one lowercase word to name it"
                ),
            );
        } else {
            self.check_words(&words, what);
        }
        let mut checked: Vec<Parameter> = Vec::new();
        for parameter in parameters {
            if checked
                .iter()
                .any(|known| known.name == parameter.name.value)
            {
                self.error(
                    parameter.name.span,
                    format!(
                        "found a second parameter named `{}`; expected each parameter once; rename one of them",
                        parameter.name.value
                    ),
                );
            }
            if parameter.name.value == "kind" {
                self.error(
                    parameter.name.span,
                    "found a parameter named `kind`; expected another name, since a variant's JSON form keeps its name in `kind`; rename it",
                );
            }
            checked.push(self.parameter(parameter));
        }
        Shape {
            text: name.text.clone(),
            parts: crate::schema::ShapePart::of(&name.parts),
            parameters: checked,
        }
    }

    fn parameter(&mut self, parameter: &ast::Parameter) -> Parameter {
        self.check_words(&parameter.name, "a parameter");
        Parameter {
            name: parameter.name.value.clone(),
            r#type: self.r#type(&parameter.r#type),
        }
    }

    fn method(&mut self, method: &'a ast::Method, found: &mut Vec<ErrorDefinition>) -> Method {
        let MethodParts {
            doc: doc_string,
            shape,
            returns,
            errors: list,
            undone_by,
            ..
        } = method_parts(method);
        let effect = match (&method.read_only, &method.idempotent) {
            (Some(_), Some(idempotent)) => {
                self.error(
                    *idempotent,
                    "found `READ ONLY IDEMPOTENT`; expected `READ ONLY` alone, since a method that changes nothing is already idempotent; remove `IDEMPOTENT`",
                );
                Effect::ReadOnly
            }
            (Some(_), None) => Effect::ReadOnly,
            (None, Some(_)) => Effect::Idempotent,
            (None, None) => Effect::Unmarked,
        };
        if let (Effect::ReadOnly, Some(undo)) = (effect, undone_by) {
            self.error(
                undo.span,
                "found `UNDONE BY` on a `READ ONLY` method; expected no `UNDONE BY`, since a method that changes nothing has nothing to undo; remove it, or the `READ ONLY`",
            );
        }
        let checked_shape = self.shape(&shape.name, &shape.arguments, "a method");
        let terms = list.and_then(|list| self.error_terms(list));
        let mut listed: Vec<Listed> = Vec::new();
        if let Some(terms) = terms {
            if let Some(name) = &terms.name {
                self.error(
                    name.span,
                    format!(
                        "found the name `{}` before a method's errors; expected its errors in brackets alone, since only an `ERRORS` group has a name, such as `` ERRORS [`missing] + file errors ``",
                        name.value
                    ),
                );
            }
            let own = self.declare_errors(terms.own, false, found);
            listed.extend(terms.own.iter().zip(own).map(|(error, index)| Listed {
                index,
                span: error.body.name.span,
            }));
            for reference in &terms.groups {
                let Some(group) = self.resolve_group(reference) else {
                    continue;
                };
                let expanded = self.expand(group, &mut Vec::new()).unwrap_or_default();
                listed.extend(expanded.into_iter().map(|index| Listed {
                    index,
                    span: reference.span,
                }));
            }
        }
        let mut errors: Vec<usize> = Vec::new();
        for Listed { index, span } in listed {
            let declared = &self.errors[index];
            let this = declared.name.clone();
            let same_words = errors
                .iter()
                .map(|known| &self.errors[*known])
                .find(|other| other.name == declared.name)
                .map(|other| other.name.clone());
            if errors.contains(&index) {
                let words = declared.name.clone();
                self.error(
                    span,
                    format!(
                        "found the error `{words}` twice among this method's errors, counting those of its groups; expected each error once; remove one"
                    ),
                );
            } else if let Some(other) = same_words {
                self.error(
                    span,
                    format!(
                        "found the errors `{other}` and `{this}` both listed for this method; expected errors of distinct words, since a response writes an error by its shape alone, without a file path; rename one of them"
                    ),
                );
            } else {
                errors.push(index);
            }
        }
        let returns = returns.map(|returns| self.parameter(returns));
        let requires = method
            .requires
            .as_ref()
            .and_then(|condition| self.condition(condition, &checked_shape, None, "REQUIRES"));
        let ensures = method.ensures.as_ref().and_then(|condition| {
            self.condition(condition, &checked_shape, returns.as_ref(), "ENSURES")
        });
        Method {
            doc: doc(doc_string),
            shape: checked_shape,
            returns,
            requires,
            ensures,
            errors,
            examples: Vec::new(),
            effect,
            undone_by: None,
        }
    }

    fn condition(
        &mut self,
        condition: &ast::Condition,
        shape: &Shape,
        returns: Option<&Parameter>,
        clause: &str,
    ) -> Option<crate::refinement::Predicate<usize>> {
        let source = self.written(condition.span);
        match crate::refinement::Predicate::method(
            &condition.predicate,
            &source[1..source.len() - 1],
            &shape.parameters,
            returns,
        ) {
            Ok(mut predicate) => {
                predicate.path = self.paths[self.file].clone();
                Some(predicate)
            }
            Err(message) => {
                self.error(condition.span, format!("invalid {clause}: {message}"));
                None
            }
        }
    }

    /// Reads `UNDONE BY move (new path: Text) to (path: Text)` of `method`:
    /// each slot one of the method's parameters, with its type, and the
    /// call one method of the protocol, by its words and the type in each
    /// slot.
    fn undo(&mut self, schema: &Schema, method: &Method, undo: &ast::Undo) -> Option<Undo> {
        let written = self
            .written(undo.span)
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        if method.effect.read_only() {
            // Reported with the method.
            return None;
        }
        let mut arguments = Vec::new();
        let mut types = Vec::new();
        for slot in &undo.arguments {
            let r#type = self.r#type(&slot.r#type);
            let Some(index) = method
                .shape
                .parameters
                .iter()
                .position(|parameter| parameter.name == slot.name.value)
            else {
                let names: Vec<String> = method
                    .shape
                    .parameters
                    .iter()
                    .map(|parameter| format!("`{}`", parameter.name))
                    .collect();
                let expected = match names.is_empty() {
                    true => "a call without slots, since the method has no parameters and an undo is called with its arguments".to_string(),
                    false => format!(
                        "one of its parameters in each slot, {}, since an undo is called with the method's own arguments",
                        names.join(", ")
                    ),
                };
                self.error(
                    slot.name.span,
                    format!(
                        "found `{}` in `UNDONE BY`, which is not a parameter of `{}`; expected {expected}",
                        slot.name.value, method.shape.text
                    ),
                );
                return None;
            };
            let parameter = &method.shape.parameters[index];
            if r#type != parameter.r#type {
                self.error(
                    slot.r#type.span(),
                    format!(
                        "found the type `{}` for `{}` in `UNDONE BY`; expected `{}`, the type of the parameter `{}`",
                        schema.type_name(&r#type),
                        parameter.name,
                        schema.type_name(&parameter.r#type),
                        parameter.name,
                    ),
                );
                return None;
            }
            arguments.push(index);
            types.push(r#type);
        }
        let same_words: Vec<(usize, &Method)> = schema
            .methods
            .iter()
            .enumerate()
            .filter(|(_, other)| other.shape.text == undo.name.text)
            .collect();
        let matching: Vec<usize> = same_words
            .iter()
            .filter(|(_, other)| {
                other
                    .shape
                    .parameters
                    .iter()
                    .map(|parameter| &parameter.r#type)
                    .eq(types.iter())
            })
            .map(|(index, _)| *index)
            .collect();
        match matching.as_slice() {
            [found] => Some(Undo {
                method: *found,
                arguments,
                text: written,
            }),
            _ if same_words.is_empty() => {
                self.error(
                    undo.name.span,
                    format!(
                        "found `UNDONE BY {written}`, but {} has no method `{}`; expected the call of a method of {}, with this method's parameters in its slots",
                        self.this_schema(),
                        undo.name.text,
                        self.this_schema(),
                    ),
                );
                None
            }
            matching => {
                let candidates: Vec<String> = same_words
                    .iter()
                    .map(|(_, other)| format!("`{}`", schema.typed_shape(&other.shape)))
                    .collect();
                let problem = match matching.is_empty() {
                    true => "matches no method".to_string(),
                    false => format!("matches {} methods", matching.len()),
                };
                self.error(
                    undo.span,
                    format!(
                        "found `UNDONE BY {written}`, which {problem}; expected the type in each slot to be the same as in exactly one of: {}",
                        candidates.join(", ")
                    ),
                );
                None
            }
        }
    }

    fn r#type(&mut self, r#type: &ast::Type) -> Type {
        match r#type {
            ast::Type::Refined(refined) => self.refined(refined),
            ast::Type::List(list) => Type::List(Box::new(self.r#type(&list.item))),
            ast::Type::Map(map) => Type::Map(Box::new(self.r#type(&map.value))),
            ast::Type::Union(union) => self.union(union),
            ast::Type::Named(reference) => self.type_use(reference),
        }
    }

    fn union(&mut self, union: &ast::UnionType) -> Type {
        let members = union
            .members
            .value
            .iter()
            .map(|member| match member {
                ast::UnionMember::Variant(variant) => Type::Variant(Box::new(self.shape(
                    &variant.body.name,
                    &variant.body.arguments,
                    "a variant",
                ))),
                ast::UnionMember::Refined(refined) => self.refined(refined),
                ast::UnionMember::Union(union) => self.union(union),
                ast::UnionMember::Named {
                    name,
                    arguments,
                    span,
                } => {
                    let (name, arguments) = self.member_reference(name, arguments);
                    self.type_use(&ast::Reference {
                        name,
                        arguments,
                        span: *span,
                    })
                }
            })
            .collect();
        self.unions.push(PendingUnion {
            r#type: Type::Union(members),
            parameters: Vec::new(),
            arguments: Vec::new(),
            written: true,
            span: union.span,
            file: self.file,
        });
        self.unions.last().expect("just pushed").r#type.clone()
    }

    /// A union's named member as a use of its type, without the doc before
    /// it: `# Where it is.` then `Text` is the call `_ Text`, its doc in
    /// the slot before the first word. A doc in any other slot is reported.
    fn member_reference(
        &mut self,
        name: &ast::CallName,
        slots: &[ast::MemberSlot],
    ) -> (ast::CallName, Vec<ast::Type>) {
        let mut parts = name.parts.clone();
        let mut slots = slots.iter().peekable();
        if let (Some(NamePart::Slot), Some(ast::MemberSlot::Doc(_))) = (parts.first(), slots.peek())
        {
            parts.remove(0);
            slots.next();
        }
        let mut arguments = Vec::new();
        for slot in slots {
            match slot {
                ast::MemberSlot::Type(r#type) => arguments.push(r#type.clone()),
                ast::MemberSlot::Doc(doc) => self.error(
                    doc.span,
                    "found a doc among the types of a union's member; expected it before the member, on the line above it",
                ),
            }
        }
        let text = ShapePart::text(&ShapePart::of(&parts));
        let name = ast::CallName {
            text,
            parts,
            span: name.span,
        };
        (name, arguments)
    }

    fn refined(&mut self, refined: &ast::RefinedType) -> Type {
        let ast::RefinedType {
            base, predicate, ..
        } = refined;
        let base = self.r#type(base);
        let source = self.written(predicate.span).to_string();
        match crate::refinement::Predicate::parse(&predicate.value, &source[1..source.len() - 1]) {
            Ok(mut predicate) => {
                predicate.path = self.paths[self.file].clone();
                Type::Refined(Box::new(base), predicate)
            }
            Err(message) => {
                self.error(predicate.span, message);
                base
            }
        }
    }

    /// The type parameter in scope that `reference` names, if it is one.
    fn parameter_named(&self, reference: &ast::Reference) -> Option<String> {
        if !reference.arguments.is_empty() || reference.name.parts.contains(&NamePart::Slot) {
            return None;
        }
        let name = spanned_words(&reference.name).value;
        self.parameters.contains(&name).then_some(name)
    }

    /// A type by its name, or a generic type by its shape with a type in
    /// each slot, `page of (file summary)`, matched to its parameters by
    /// position.
    fn type_use(&mut self, reference: &ast::Reference) -> Type {
        if let Some(parameter) = self.parameter_named(reference) {
            return Type::Parameter(parameter);
        }
        if let Some(built_in) = crate::prelude::reference(reference) {
            return built_in;
        }
        let Some((path, arguments)) = self.path(reference) else {
            return Type::Text;
        };
        let arguments: Vec<Type> = arguments
            .iter()
            .map(|argument| self.r#type(argument))
            .collect();
        if let Some(alias) = self
            .aliases
            .iter()
            .position(|alias| alias.declared.name == path.name)
        {
            let known = &self.aliases[alias].declared;
            if !self.accessible(known) {
                let (name, file, public) = (known.name.clone(), known.file, known.public);
                self.unavailable(reference.span, &name, file, public);
            }
            let parameters: Vec<String> = self.aliases[alias]
                .parameters
                .iter()
                .map(|parameter| parameter.value.clone())
                .collect();
            let Some(r#type) = self.alias_type(alias) else {
                return Type::Text;
            };
            if !arguments.is_empty() {
                self.unions.push(PendingUnion {
                    r#type: r#type.clone(),
                    parameters: parameters.clone(),
                    arguments: arguments.clone(),
                    written: false,
                    span: reference.span,
                    file: self.file,
                });
            }
            return r#type.substitute(&parameters, &arguments);
        }
        let Some(index) = self.resolve_type(reference.span, &path, arguments.len()) else {
            return Type::Text;
        };
        match arguments.is_empty() {
            true => Type::Named(index),
            false => {
                let r#type = Type::Generic(index, arguments);
                self.unions.push(PendingUnion {
                    r#type: r#type.clone(),
                    parameters: Vec::new(),
                    arguments: Vec::new(),
                    written: false,
                    span: reference.span,
                    file: self.file,
                });
                r#type
            }
        }
    }

    /// A type that holds itself, other than through a `LIST` or a `MAP`, has
    /// no finite value. A named type holds the types in its slots, and a
    /// use of a generic type holds the arguments of the parameters that the
    /// generic type holds, so the rule holds for every use.
    /// Reports an alias of a union of variants that leads back to itself,
    /// through lists, maps or other such aliases: an alias is only another
    /// name for its type, so it would stand for a type without end. A
    /// `RECORD` or a `NAMED TYPE` holds values of its own kind instead.
    fn check_variants_recursion(&mut self, schema: &Schema) {
        fn reached(r#type: &Type, schema: &Schema, found: &mut Vec<usize>) {
            match r#type {
                Type::Refined(base, _) | Type::List(base) | Type::Map(base) => {
                    reached(base, schema, found);
                }
                Type::Named(index) | Type::Generic(index, _) => {
                    if let Type::Generic(_, arguments) = r#type {
                        for argument in arguments {
                            reached(argument, schema, found);
                        }
                    }
                    if found.contains(index) {
                        return;
                    }
                    if let TypeKind::Variants(variants) = &schema.types[*index].kind {
                        found.push(*index);
                        for parameter in variants.iter().flat_map(|v| &v.shape.parameters) {
                            reached(&parameter.r#type, schema, found);
                        }
                    }
                }
                Type::Union(members) | Type::Tuple(members) => {
                    for member in members {
                        reached(member, schema, found);
                    }
                }
                Type::Variant(shape) => {
                    for parameter in &shape.parameters {
                        reached(&parameter.r#type, schema, found);
                    }
                }
                _ => {}
            }
        }
        for (index, definition) in schema.types.iter().enumerate() {
            let TypeKind::Variants(variants) = &definition.kind else {
                continue;
            };
            let mut found = Vec::new();
            for parameter in variants.iter().flat_map(|v| &v.shape.parameters) {
                reached(&parameter.r#type, schema, &mut found);
            }
            if found.contains(&index) {
                let declared = &self.types[index];
                let (file, span, name) = (declared.file, declared.span, declared.name.clone());
                self.within(file, |checker| {
                    checker.error(
                        span,
                        format!(
                            "found the type alias `{name}`, a union of variants that leads back to itself; expected an alias of a type that doesn't, since an alias is only another name for its type; to hold values of its own kind, use a `RECORD` or a `NAMED TYPE` that holds them through a `LIST`"
                        ),
                    );
                });
            }
        }
    }

    fn check_recursion(&mut self, schema: &Schema) {
        /// The types `type` holds, other than through a `LIST` or a `MAP`,
        /// and the type parameters it holds.
        fn held<'t>(
            r#type: &'t Type,
            holds_parameters: &[Vec<String>],
            schema: &Schema,
            types: &mut Vec<usize>,
            parameters: &mut Vec<&'t str>,
        ) {
            match r#type {
                Type::Refined(base, _) => held(base, holds_parameters, schema, types, parameters),
                Type::Named(index) => types.push(*index),
                Type::Generic(index, arguments) => {
                    types.push(*index);
                    for (parameter, argument) in
                        schema.types[*index].parameters.iter().zip(arguments)
                    {
                        if holds_parameters[*index].contains(parameter) {
                            held(argument, holds_parameters, schema, types, parameters);
                        }
                    }
                }
                Type::Parameter(name) => parameters.push(name),
                Type::Union(members) => {
                    for member in members {
                        held(member, holds_parameters, schema, types, parameters);
                    }
                }
                Type::Variant(shape) => {
                    for parameter in &shape.parameters {
                        held(
                            &parameter.r#type,
                            holds_parameters,
                            schema,
                            types,
                            parameters,
                        );
                    }
                }
                _ => {}
            }
        }
        fn direct(definition: &TypeDefinition) -> Vec<&Type> {
            match &definition.kind {
                TypeKind::Record(fields) => fields.iter().map(|field| &field.r#type).collect(),
                TypeKind::Variants(variants) => variants
                    .iter()
                    .flat_map(|variant| &variant.shape.parameters)
                    .map(|parameter| &parameter.r#type)
                    .collect(),
                TypeKind::Distinct { underlying, .. } => vec![underlying],
            }
        }
        // Which of its parameters each generic type holds, found by
        // repeating until nothing changes, since generic types hold each
        // other.
        let mut holds_parameters: Vec<Vec<String>> = vec![Vec::new(); schema.types.len()];
        loop {
            let found: Vec<Vec<String>> = schema
                .types
                .iter()
                .map(|definition| {
                    let mut parameters = Vec::new();
                    for r#type in direct(definition) {
                        held(
                            r#type,
                            &holds_parameters,
                            schema,
                            &mut Vec::new(),
                            &mut parameters,
                        );
                    }
                    let mut parameters: Vec<String> =
                        parameters.into_iter().map(str::to_string).collect();
                    parameters.sort();
                    parameters.dedup();
                    parameters
                })
                .collect();
            if found == holds_parameters {
                break;
            }
            holds_parameters = found;
        }
        let edges: Vec<Vec<usize>> = schema
            .types
            .iter()
            .map(|definition| {
                let mut types = Vec::new();
                for r#type in direct(definition) {
                    held(
                        r#type,
                        &holds_parameters,
                        schema,
                        &mut types,
                        &mut Vec::new(),
                    );
                }
                types
            })
            .collect();
        for start in cycles(&edges) {
            let name = self.type_shown(start);
            let declared = &self.types[start];
            let (file, span) = (declared.file, declared.span);
            self.file = file;
            self.error(
                span,
                format!(
                    "found the type `{name}` holding itself; expected a type that holds itself to do so through a `LIST` or a `MAP`, so that its values end; make the field a `LIST`"
                ),
            );
        }
        self.check_generic_recursion(schema);
    }

    /// A generic type that uses itself, directly or through other generic
    /// types, must pass its own parameters as they are: `nest of (Item)`
    /// within `nest of (Item: TYPE)`. With other arguments, such as
    /// `nest of (LIST: Item)`, each use would need another, without end.
    fn check_generic_recursion(&mut self, schema: &Schema) {
        /// The generic types `type` uses, anywhere in it, with their
        /// arguments.
        fn uses<'t>(r#type: &'t Type, found: &mut Vec<(usize, &'t [Type])>) {
            match r#type {
                Type::Generic(index, arguments) => {
                    found.push((*index, arguments));
                    arguments.iter().for_each(|argument| uses(argument, found));
                }
                Type::List(item) | Type::Map(item) | Type::Refined(item, _) => uses(item, found),
                Type::Tuple(slots) | Type::Union(slots) => {
                    slots.iter().for_each(|slot| uses(slot, found))
                }
                Type::Variant(shape) => shape
                    .parameters
                    .iter()
                    .for_each(|parameter| uses(&parameter.r#type, found)),
                _ => {}
            }
        }
        /// Follows the uses of generic types from `type` with `arguments`,
        /// and returns the first use of `start` with arguments other than
        /// its own parameters. A type met again on `path` with other
        /// arguments is not followed: it changes its own arguments, and is
        /// reported for itself.
        fn changed(
            schema: &Schema,
            start: usize,
            r#type: usize,
            arguments: &[Type],
            path: &mut Vec<(usize, Vec<Type>)>,
            seen: &mut Vec<(usize, Vec<Type>)>,
        ) -> Option<Type> {
            let definition = &schema.types[r#type];
            let kind = definition
                .kind
                .substitute(&definition.parameters, arguments);
            let mut found = Vec::new();
            match &kind {
                TypeKind::Record(fields) => fields
                    .iter()
                    .for_each(|field| uses(&field.r#type, &mut found)),
                TypeKind::Variants(variants) => variants
                    .iter()
                    .flat_map(|variant| &variant.shape.parameters)
                    .for_each(|parameter| uses(&parameter.r#type, &mut found)),
                TypeKind::Distinct { underlying, .. } => uses(underlying, &mut found),
            }
            for (index, used) in found {
                let own: Vec<Type> = schema.types[index]
                    .parameters
                    .iter()
                    .map(|name| Type::Parameter(name.clone()))
                    .collect();
                if index == start {
                    if used != own.as_slice() {
                        return Some(Type::Generic(index, used.to_vec()));
                    }
                    continue;
                }
                let at = (index, used.to_vec());
                if seen.contains(&at) || path.iter().any(|(known, _)| *known == index) {
                    continue;
                }
                seen.push(at.clone());
                path.push(at);
                let inner = changed(schema, start, index, used, path, seen);
                path.pop();
                if inner.is_some() {
                    return inner;
                }
            }
            None
        }
        for (start, definition) in schema.types.iter().enumerate() {
            if definition.parameters.is_empty() {
                continue;
            }
            let own: Vec<Type> = definition
                .parameters
                .iter()
                .map(|name| Type::Parameter(name.clone()))
                .collect();
            let Some(changed) = changed(
                schema,
                start,
                start,
                &own,
                &mut vec![(start, own.clone())],
                &mut Vec::new(),
            ) else {
                continue;
            };
            let name = self.type_shown(start);
            let declared = &self.types[start];
            let (file, span) = (declared.file, declared.span);
            self.file = file;
            self.error(
                span,
                format!(
                    "found the generic type `{name}` using itself as `{}`; expected each use of `{name}` within it to pass its own parameters as they are, `{}`, so that every use of `{name}` holds the same types all the way down",
                    schema.type_name(&changed),
                    schema.type_name(&Type::Generic(start, own)),
                ),
            );
        }
    }
}

/// The nodes of `edges` that lead back to themselves, in order.
fn cycles(edges: &[Vec<usize>]) -> Vec<usize> {
    (0..edges.len())
        .filter(|start| {
            let mut seen = vec![false; edges.len()];
            let mut pending = edges[*start].clone();
            while let Some(next) = pending.pop() {
                if next == *start {
                    return true;
                }
                if !std::mem::replace(&mut seen[next], true) {
                    pending.extend(&edges[next]);
                }
            }
            false
        })
        .collect()
}
