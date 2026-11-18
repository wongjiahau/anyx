//! Checks flat names, direct import visibility, types and method shapes.

use anyx::{NamePart, Node, NodeKind, Span, StringKind, StringLiteral};

use crate::codec;
use crate::response::{self, WrittenOutcome};

use crate::error_terms::{self, ErrorTerms, GroupReference, Misplaced};
use crate::grammar as ast;
use crate::schema::{
    DecodedFile, Diagnostic, Effect, ErrorDefinition, Example, Field, Method, Parameter, Schema,
    Shape, Shaped, SourceFile, Type, TypeDefinition, TypeKind, Undo, fill_slots,
};

/// A type, error or alias, where it is declared.
struct Declared {
    name: String,
    span: Span,
    /// The index of its file.
    file: usize,
    public: bool,
}

/// An `ERRORS` group: `ERRORS file errors [trashed, missing] + other`.
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
    node: anyx::Node,
    file: usize,
}

/// An error of a method, and where its list names it.
struct Listed {
    index: usize,
    span: Span,
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
        intrinsic: choice.intrinsic,
        public: choice.public.is_some(),
        body: type_shape(&choice.body.name, parts),
    }
}

fn named_type(named: &ast::NamedType) -> Declaration<'_, &ast::NamedTypeBody> {
    Declaration {
        doc: &named.doc,
        intrinsic: named.intrinsic,
        public: named.public.is_some(),
        body: &named.body,
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

/// The name a `TYPE` declares, with or without the type it names.
fn named_type_name(body: &ast::NamedTypeBody) -> &ast::Spanned<String> {
    match body {
        ast::NamedTypeBody::Typed { name, .. } | ast::NamedTypeBody::Bare { name, .. } => name,
    }
}

/// A slot of a `RECORD`'s or `CHOICE`'s name, as written: the braces of its
/// fields or variants, or a type parameter.
enum Part<'b, T> {
    Body(&'b ast::Spanned<T>),
    Parameter(&'b ast::TypeParameter),
}

/// The name of a `RECORD` or `CHOICE` read as a shape: `page of (item: Type)
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
    /// The type parameters of the generic record or choice being checked,
    /// which its fields and variants may use as types.
    parameters: Vec<String>,
    groups: Vec<Group<'a>>,
    /// Each error, in the order of the schema's errors.
    errors: Vec<Declared>,
    defaults: Vec<PendingDefault>,
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
                ast::Definition::NamedType(d) => Some(named_type_name(&d.body).value.clone()),
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
                        self.check_intrinsic(found.intrinsic);
                        self.declare_shaped_type(&found.body, found.public, "CHOICE", "variants");
                    }
                    ast::Definition::NamedType(found) => {
                        let found = named_type(found);
                        self.check_intrinsic(found.intrinsic);
                        let name = named_type_name(found.body);
                        if let ast::NamedTypeBody::Bare { name, span } = found.body {
                            self.error(
                                *span,
                                format!(
                                    "found `TYPE {name}` without a type; expected the type it names after a `:`, as in `TYPE {name}: Text`",
                                    name = name.value
                                ),
                            );
                        }
                        self.check_type_words(name, false);
                        self.declare_type(name, found.public);
                        self.declare_type_parameters(&[]);
                    }
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
                            checker.variants(variants)
                        });
                        let kind = TypeKind::Choice(variants);
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
                    ast::Definition::NamedType(found) => {
                        let found = named_type(found);
                        let underlying = match found.body {
                            ast::NamedTypeBody::Typed { r#type, .. } => self.r#type(r#type),
                            // Reported when it was declared.
                            ast::NamedTypeBody::Bare { .. } => Type::Text,
                        };
                        types.push(TypeDefinition {
                            public: found.public || self.file == self.prelude_file,
                            doc: doc(found.doc),
                            name: named_type_name(found.body).value.clone(),
                            parameters: Vec::new(),
                            kind: TypeKind::Distinct(underlying),
                        });
                    }
                    ast::Definition::Errors(_)
                    | ast::Definition::Method(_)
                    | ast::Definition::Function(_)
                    | ast::Definition::Import(_) => {}
                }
            }
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
        if self.diagnostics.is_empty() {
            self.diagnostics
                .extend(crate::refinement::check_schema(&schema));
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

    /// Reads each field's `DEFAULT` as a value of the field's type. A
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
                let read = crate::codec::decode(schema, &field.r#type, &default.node);
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
                    "found a `{keyword}` whose name has no words; expected at least one word to name it, with any type parameters among the words, as in `{keyword} Page of (item: Type)`"
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
                .map(|parameter| format!("{parameter}: Type"))
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
                    "found braces in the name of a `{keyword}`, before its end; expected its {what} in one pair of braces, last, and only type parameters such as `(item: Type)` in the name's other slots"
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
    /// shows it: `page of (item)`.
    fn type_shown(&self, index: usize) -> String {
        fill_slots(&self.types[index].name, &self.parameter_names(index))
    }

    /// Declares the type parameters in the slots of the type just declared,
    /// each a name once.
    fn declare_type_parameters(&mut self, written: &[&ast::TypeParameter]) {
        let mut names: Vec<ast::Spanned<String>> = Vec::new();
        for parameter in written {
            self.check_words(&parameter.name, "a type parameter");
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
        self.type_parameters.push(names);
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
            TypeKind::Choice(variants) => variants
                .iter()
                .flat_map(|variant| &variant.shape.parameters)
                .map(|parameter| &parameter.r#type)
                .collect(),
            TypeKind::Distinct(underlying) => vec![underlying],
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
                "found an `ERRORS` without a name before its errors; expected a group's name, its own errors in brackets, then any groups it joins with `+`, such as `ERRORS file errors [trashed, missing]`",
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
                        error.name.span,
                        format!(
                            "found a second error `{words}` in {}, after the one on line {line}; expected each error once in the protocol; to share one, put it in an `ERRORS` group and join the group with `+`",
                            self.this_schema()
                        ),
                    );
                }
                let declared = self.declared(
                    &ast::Spanned {
                        value: words,
                        span: error.name.span,
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
        if !same_words.is_empty() {
            let count = match arguments {
                0 => "no type in a slot".to_string(),
                1 => "a type in 1 slot".to_string(),
                count => format!("types in {count} slots"),
            };
            self.error(
                span,
                format!(
                    "found `{written}`, with {count}, but {scope} has no type of that shape; the types with its words are {}; expected one of them, with a type in parentheses in each slot it has",
                    shown(self, &same_words)
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
            "found `{written}`, which is not a type of {scope}; expected a built-in type ({built_in}), `LIST: type`, `MAP: type`, or a `RECORD`, `CHOICE` or `TYPE` defined here; define `{written}`, or fix the name; import the declaration directly with `USE` if it is in another file{similar}",
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
        for field in fields {
            self.check_words(&field.name, "a field");
            if checked.iter().any(|known| known.name == field.name.value) {
                self.error(
                    field.name.span,
                    format!(
                        "found a second field named `{}`; expected each field once; rename or remove one of them",
                        field.name.value
                    ),
                );
            }
            let r#type = match &field.r#type {
                ast::FieldType::Defaulted(defaulted) => {
                    let (r#type, node) = self.defaulted(defaulted);
                    self.defaults.push(PendingDefault {
                        r#type: record,
                        field: checked.len(),
                        node: node.clone(),
                        file: self.file,
                    });
                    r#type
                }
                ast::FieldType::Type(r#type) => self.r#type(r#type),
            };
            checked.push(Field {
                doc: doc(&field.doc),
                name: field.name.value.clone(),
                r#type,
                default: None,
            });
        }
        checked
    }

    fn variants(&mut self, variants: &[ast::Variant]) -> Vec<Shaped> {
        let mut checked: Vec<Shaped> = Vec::new();
        for variant in variants {
            let shaped = self.shaped(variant, "a variant");
            if checked
                .iter()
                .any(|known| known.shape.words() == shaped.shape.words())
            {
                self.error(
                    variant.name.span,
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

    /// A variant or an error: its doc, then its shape.
    fn shaped(&mut self, variant: &ast::Variant, what: &str) -> Shaped {
        let mut slots = variant.arguments.iter().peekable();
        // A doc fills the call's first slot, before the first word.
        let doc = match (variant.name.parts.first(), slots.peek()) {
            (Some(NamePart::Slot), Some(ast::VariantSlot::Doc(doc))) => {
                slots.next();
                Some(doc.value.value.clone())
            }
            _ => None,
        };
        let mut parameters = Vec::new();
        for slot in slots {
            match slot {
                ast::VariantSlot::Doc(doc) => self.error(
                    doc.span,
                    format!(
                        "found a string in {what}'s slot; expected a parameter, as in `(revision: Integer)`; {what}'s `#` doc goes on the lines before it"
                    ),
                ),
                ast::VariantSlot::Parameter(parameter) => parameters.push(parameter.clone()),
                ast::VariantSlot::Unnamed { name, .. } => {
                    let reference = ast::Reference {
                        name: ast::CallName { text: name.value.clone(), parts: name.value.split(' ').map(|text| NamePart::Word { text: text.into(), span: name.span }).collect(), span: name.span },
                        arguments: Vec::new(), span: name.span,
                    };
                    parameters.push(ast::Parameter { name: name.clone(), r#type: ast::Type::Named(reference), span: name.span });
                }
            }
        }
        let mut name = variant.name.clone();
        if doc.is_some() {
            name.parts.remove(0);
            name.text = name.text.trim_start_matches('_').trim_start().to_string();
        }
        let shape = self.shape(&name, &parameters, what);
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
                        "found the name `{}` before a method's errors; expected its errors in brackets alone, since only an `ERRORS` group has a name, such as `ERRORS [missing] + file errors`",
                        name.value
                    ),
                );
            }
            let own = self.declare_errors(terms.own, false, found);
            listed.extend(terms.own.iter().zip(own).map(|(error, index)| Listed {
                index,
                span: error.name.span,
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
            ast::Type::Refined {
                base, predicate, ..
            } => {
                let base = self.r#type(base);
                let source = self.written(predicate.span).to_string();
                match crate::refinement::Predicate::parse(
                    &predicate.value,
                    &source[1..source.len() - 1],
                ) {
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
            ast::Type::List { item, .. } => Type::List(Box::new(self.r#type(item))),
            ast::Type::Map { value, .. } => Type::Map(Box::new(self.r#type(value))),
            ast::Type::Named(reference) => self.type_use(reference),
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
        let Some(index) = self.resolve_type(reference.span, &path, arguments.len()) else {
            return Type::Text;
        };
        match arguments.is_empty() {
            true => Type::Named(index),
            false => Type::Generic(index, arguments),
        }
    }

    /// The type of a field with a `DEFAULT`, and the default as written.
    fn defaulted<'d>(&mut self, defaulted: &'d ast::Defaulted) -> (Type, &'d anyx::Node) {
        match defaulted {
            ast::Defaulted::List { item, .. } => {
                let (item, default) = self.defaulted(item);
                (Type::List(Box::new(item)), default)
            }
            ast::Defaulted::ListOfNamed { item, default, .. } => {
                (Type::List(Box::new(self.r#type(item))), default)
            }
            ast::Defaulted::Map { value, .. } => {
                let (value, default) = self.defaulted(value);
                (Type::Map(Box::new(value)), default)
            }
            ast::Defaulted::MapOfNamed { value, default, .. } => {
                (Type::Map(Box::new(self.r#type(value))), default)
            }
            ast::Defaulted::Other {
                r#type, default, ..
            } => (self.r#type(r#type), default),
        }
    }

    /// A named type that leads back to itself, as in `TYPE a: LIST: a` or
    /// `TYPE a: b, TYPE b: a`, names no type at all. Returns the types it
    /// reported.
    fn check_named_cycles(&mut self, schema: &Schema) -> Vec<usize> {
        fn mentioned(r#type: &Type, found: &mut Vec<usize>) {
            match r#type {
                Type::Named(index) => found.push(*index),
                Type::List(item) | Type::Map(item) | Type::Refined(item, _) => {
                    mentioned(item, found)
                }
                Type::Generic(index, arguments) => {
                    found.push(*index);
                    arguments
                        .iter()
                        .for_each(|argument| mentioned(argument, found));
                }
                _ => {}
            }
        }
        let edges: Vec<Vec<usize>> = schema
            .types
            .iter()
            .map(|definition| {
                let mut found = Vec::new();
                if let TypeKind::Distinct(underlying) = &definition.kind {
                    mentioned(underlying, &mut found);
                }
                found
                    .into_iter()
                    .filter(|index| matches!(schema.types[*index].kind, TypeKind::Distinct(_)))
                    .collect()
            })
            .collect();
        let cycles = cycles(&edges);
        for start in &cycles {
            let declared = &self.types[*start];
            let (file, span, name) = (declared.file, declared.span, declared.name.clone());
            self.file = file;
            self.error(
                span,
                format!(
                    "found the named type `{name}` referring to itself; expected a `TYPE` over a type that doesn't lead back to it, such as `TYPE {name}: Text`; to hold values of its own kind, name a `RECORD` or `CHOICE` that holds them through a `LIST`"
                ),
            );
        }
        cycles
    }

    /// A type that holds itself, other than through a `LIST` or a `MAP`, has
    /// no finite value. A named type holds the type underneath it, and a
    /// use of a generic type holds the arguments of the parameters that the
    /// generic type holds, so the rule holds for every use.
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
                _ => {}
            }
        }
        fn direct(definition: &TypeDefinition) -> Vec<&Type> {
            match &definition.kind {
                TypeKind::Record(fields) => fields.iter().map(|field| &field.r#type).collect(),
                TypeKind::Choice(variants) => variants
                    .iter()
                    .flat_map(|variant| &variant.shape.parameters)
                    .map(|parameter| &parameter.r#type)
                    .collect(),
                TypeKind::Distinct(underlying) => vec![underlying],
            }
        }
        let reported = self.check_named_cycles(schema);
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
            if reported.contains(&start) {
                continue;
            }
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
    /// types, must pass its own parameters as they are: `nest of (item)`
    /// within `nest of (item: Type)`. With other arguments, such as
    /// `nest of (LIST: item)`, each use would need another, without end.
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
                TypeKind::Choice(variants) => variants
                    .iter()
                    .flat_map(|variant| &variant.shape.parameters)
                    .for_each(|parameter| uses(&parameter.r#type, &mut found)),
                TypeKind::Distinct(underlying) => uses(underlying, &mut found),
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
