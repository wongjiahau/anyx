//! Checks a decoded schema: resolves every name in the scope it is written
//! in, keeps private types and errors to their scope, and reports what is
//! defined twice.

use anyx::{NamePart, Node, NodeKind, Span, StringLiteral};

use crate::codec;
use crate::response::{self, WrittenOutcome};

use crate::error_terms::{self, ErrorTerms, GroupReference, Misplaced};
use crate::grammar as ast;
use crate::schema::{
    DecodedFile, Diagnostic, ErrorDefinition, Example, Field, Method, Parameter, ROOT_FILE, Schema,
    Shape, Shaped, SourceFile, Type, TypeDefinition, TypeKind, scope_file, scoped,
};

/// A type, error or alias, where it is declared.
struct Declared {
    scope: Vec<String>,
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
    scope: Vec<String>,
}

/// An error of a method, and where its list names it.
struct Listed {
    index: usize,
    span: Span,
}

/// A reference read as a path.
struct Path {
    scope: Vec<String>,
    name: String,
    /// Written as a full path, `common/diagnostic`, rather than a name.
    full: bool,
}

/// What a `RECORD`, `CHOICE` or `ERRORS` group declares, public or not.
struct Declaration<'b, T> {
    doc: &'b Option<ast::Spanned<StringLiteral>>,
    public: bool,
    body: &'b T,
    /// The type parameters after `GENERIC`, for a generic record or choice.
    parameters: Option<&'b ast::TypeParameters>,
}

fn record(record: &ast::Record) -> Declaration<'_, ast::RecordBody> {
    match record {
        ast::Record::Public { doc, body, .. } => Declaration {
            doc,
            public: true,
            body,
            parameters: None,
        },
        ast::Record::Private { doc, body, .. } => Declaration {
            doc,
            public: false,
            body,
            parameters: None,
        },
        ast::Record::PublicGeneric {
            doc,
            parameters,
            body,
            ..
        } => Declaration {
            doc,
            public: true,
            body,
            parameters: Some(parameters),
        },
        ast::Record::Generic {
            doc,
            parameters,
            body,
            ..
        } => Declaration {
            doc,
            public: false,
            body,
            parameters: Some(parameters),
        },
    }
}

fn choice(choice: &ast::Choice) -> Declaration<'_, ast::ChoiceBody> {
    match choice {
        ast::Choice::Public { doc, body, .. } => Declaration {
            doc,
            public: true,
            body,
            parameters: None,
        },
        ast::Choice::Private { doc, body, .. } => Declaration {
            doc,
            public: false,
            body,
            parameters: None,
        },
        ast::Choice::PublicGeneric {
            doc,
            parameters,
            body,
            ..
        } => Declaration {
            doc,
            public: true,
            body,
            parameters: Some(parameters),
        },
        ast::Choice::Generic {
            doc,
            parameters,
            body,
            ..
        } => Declaration {
            doc,
            public: false,
            body,
            parameters: Some(parameters),
        },
    }
}

fn named_type(named: &ast::NamedType) -> Declaration<'_, ast::NamedTypeBody> {
    match named {
        ast::NamedType::Public { doc, body, .. } => Declaration {
            doc,
            public: true,
            body,
            parameters: None,
        },
        ast::NamedType::Private { doc, body, .. } => Declaration {
            doc,
            public: false,
            body,
            parameters: None,
        },
    }
}

fn errors(errors: &ast::Errors) -> Declaration<'_, ast::ErrorList> {
    match errors {
        ast::Errors::Public { doc, body, .. } => Declaration {
            doc,
            public: true,
            body,
            parameters: None,
        },
        ast::Errors::Private { doc, body, .. } => Declaration {
            doc,
            public: false,
            body,
            parameters: None,
        },
    }
}

/// A method's clauses, whichever of its forms it is written in.
struct MethodParts<'b> {
    doc: &'b Option<ast::Spanned<StringLiteral>>,
    shape: &'b ast::Shape,
    /// `None` for `RETURNS NOTHING`.
    returns: Option<&'b ast::Returns>,
    errors: Option<&'b ast::ErrorList>,
    examples: Option<&'b ast::Examples>,
}

fn method_parts(method: &ast::Method) -> MethodParts<'_> {
    match method {
        ast::Method::ValueWithErrorsAndExamples {
            doc,
            shape,
            returns,
            errors,
            examples,
            ..
        } => MethodParts {
            doc,
            shape,
            returns: Some(returns),
            errors: Some(errors),
            examples: Some(examples),
        },
        ast::Method::ValueWithErrors {
            doc,
            shape,
            returns,
            errors,
            ..
        } => MethodParts {
            doc,
            shape,
            returns: Some(returns),
            errors: Some(errors),
            examples: None,
        },
        ast::Method::ValueWithExamples {
            doc,
            shape,
            returns,
            examples,
            ..
        } => MethodParts {
            doc,
            shape,
            returns: Some(returns),
            errors: None,
            examples: Some(examples),
        },
        ast::Method::Value {
            doc,
            shape,
            returns,
            ..
        } => MethodParts {
            doc,
            shape,
            returns: Some(returns),
            errors: None,
            examples: None,
        },
        ast::Method::NothingWithErrorsAndExamples {
            doc,
            shape,
            errors,
            examples,
            ..
        } => MethodParts {
            doc,
            shape,
            returns: None,
            errors: Some(errors),
            examples: Some(examples),
        },
        ast::Method::NothingWithErrors {
            doc, shape, errors, ..
        } => MethodParts {
            doc,
            shape,
            returns: None,
            errors: Some(errors),
            examples: None,
        },
        ast::Method::NothingWithExamples {
            doc,
            shape,
            examples,
            ..
        } => MethodParts {
            doc,
            shape,
            returns: None,
            errors: None,
            examples: Some(examples),
        },
        ast::Method::Nothing { doc, shape, .. } => MethodParts {
            doc,
            shape,
            returns: None,
            errors: None,
            examples: None,
        },
    }
}

/// A method's `EXAMPLES`, checked once the schema they are checked against
/// is read.
struct PendingExamples<'a> {
    /// The method's index in the schema.
    method: usize,
    file: usize,
    scope: Vec<String>,
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
    sources: Vec<String>,
    /// The file being checked, which diagnostics are in.
    file: usize,
    /// Its scope, which names are resolved in.
    scope: Vec<String>,
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

    /// Checks within `file` and its `scope`, then goes back.
    fn within<T>(
        &mut self,
        file: usize,
        scope: &[String],
        check: impl FnOnce(&mut Self) -> T,
    ) -> T {
        let file = std::mem::replace(&mut self.file, file);
        let scope = std::mem::replace(&mut self.scope, scope.to_vec());
        let checked = check(self);
        self.file = file;
        self.scope = scope;
        checked
    }

    /// The source at `span` in the file being checked.
    fn written(&self, span: Span) -> &str {
        &self.sources[self.file][span.start..span.end]
    }

    /// The scope being checked, in a message: `this schema` for one of a
    /// single file, else `the scope `files``.
    fn this_scope(&self) -> String {
        match self.scope.is_empty() {
            true => "this schema".to_string(),
            false => format!("the scope `{}`", self.scope.join("/")),
        }
    }

    pub(crate) fn schema(&mut self, files: Vec<SourceFile>, decoded: &'a [DecodedFile]) -> Schema {
        self.paths = files.iter().map(|file| file.path.clone()).collect();
        self.sources = files.iter().map(|file| file.source.clone()).collect();
        for (index, file) in decoded.iter().enumerate() {
            self.within(index, &file.scope, |checker| {
                checker.header(file, decoded.len() > 1)
            });
        }
        for (index, file) in decoded.iter().enumerate() {
            self.file = index;
            self.scope = file.scope.clone();
            for definition in &file.document.definition {
                match definition {
                    ast::Definition::Record(found) => {
                        let found = record(found);
                        self.declare_type(&found.body.name, found.public);
                        self.declare_type_parameters(found.parameters);
                    }
                    ast::Definition::Choice(found) => {
                        let found = choice(found);
                        self.declare_type(&found.body.name, found.public);
                        self.declare_type_parameters(found.parameters);
                    }
                    ast::Definition::NamedType(found) => {
                        let found = named_type(found);
                        self.declare_type(&found.body.name, found.public);
                        self.declare_type_parameters(None);
                    }
                    ast::Definition::Errors(found) => {
                        let found = errors(found);
                        self.declare_group(found.body, found.public);
                    }
                    ast::Definition::Method(_) => {}
                }
            }
        }
        let mut types = Vec::new();
        let mut errors_found = Vec::new();
        let mut methods: Vec<Method> = Vec::new();
        for (index, file) in decoded.iter().enumerate() {
            self.file = index;
            self.scope = file.scope.clone();
            for definition in &file.document.definition {
                match definition {
                    ast::Definition::Record(found) => {
                        let found = record(found);
                        let index = types.len();
                        let fields = self.generic(index, |checker| {
                            checker.fields(index, &found.body.fields.value)
                        });
                        let kind = TypeKind::Record(fields);
                        self.check_parameters_used(index, &kind);
                        types.push(TypeDefinition {
                            scope: file.scope.clone(),
                            public: found.public,
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
                            checker.variants(&found.body.variants.value)
                        });
                        let kind = TypeKind::Choice(variants);
                        self.check_parameters_used(index, &kind);
                        types.push(TypeDefinition {
                            scope: file.scope.clone(),
                            public: found.public,
                            doc: doc(found.doc),
                            name: found.body.name.value.clone(),
                            parameters: self.parameter_names(index),
                            kind,
                        });
                    }
                    ast::Definition::NamedType(found) => {
                        let found = named_type(found);
                        types.push(TypeDefinition {
                            scope: file.scope.clone(),
                            public: found.public,
                            doc: doc(found.doc),
                            name: found.body.name.value.clone(),
                            parameters: Vec::new(),
                            kind: TypeKind::Distinct(self.r#type(&found.body.r#type)),
                        });
                    }
                    ast::Definition::Errors(_) | ast::Definition::Method(_) => {}
                }
            }
        }
        for index in 0..self.groups.len() {
            let declared = &self.groups[index].declared;
            let (file, scope, public) = (declared.file, declared.scope.clone(), declared.public);
            let own = self.groups[index].own;
            let own_errors = self.within(file, &scope, |checker| {
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
            self.scope = file.scope.clone();
            for definition in &file.document.definition {
                if let ast::Definition::Method(found) = definition {
                    let method = self.method(found, &mut errors_found);
                    let parts = method_parts(found);
                    if let Some(examples) = parts.examples {
                        self.examples.push(PendingExamples {
                            method: methods.len(),
                            file: index,
                            scope: file.scope.clone(),
                            examples,
                        });
                    }
                    if methods.iter().any(|other| {
                        other.scope == method.scope && other.shape.text == method.shape.text
                    }) {
                        self.error(
                                parts.shape.name.span,
                                format!(
                                    "found a second method `{}`; expected each method shape once in a scope; rename or remove one of them",
                                    method.shape.text
                                ),
                            );
                    }
                    methods.push(method);
                }
            }
        }
        let header = decoded
            .first()
            .and_then(|root| root.document.header.as_ref())
            .filter(|_| decoded[0].scope.is_empty());
        let name = header.map(|header| {
            self.within(0, &[], |checker| {
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
            types,
            errors: errors_found,
            methods,
        };
        self.check_recursion(&schema);
        if self.diagnostics.is_empty() {
            self.read_defaults(&mut schema);
        }
        // Examples are read against the schema, defaults and all, which
        // must be whole first: a mistake elsewhere would only show up again
        // in them.
        if self.diagnostics.is_empty() {
            for pending in std::mem::take(&mut self.examples) {
                let examples = self.within(pending.file, &pending.scope, |checker| {
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
                        self.within(default.file, &default.scope, |checker| {
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
                let (doc, call) = self.without_doc(call);
                let read = codec::decode_call_of(schema, method, &call).and_then(|arguments| {
                    response::read_outcome(schema, method, written, span)
                        .map(|outcome| (arguments, outcome))
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
    fn without_doc(&self, node: &Node) -> (Option<String>, Node) {
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
                if self.sources[self.file][first.span.start..].starts_with('#')
                    && call.name.len() > 1 =>
            {
                (literal.value.clone(), None)
            }
            NodeKind::Call(_) => match self.without_doc(first) {
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

    /// The root has the `PROTOCOL` header, alone when there are scope files;
    /// a scope file has none.
    fn header(&mut self, file: &DecodedFile, several: bool) {
        let path = self.paths[self.file].clone();
        match (&file.document.header, file.scope.is_empty()) {
            (Some(_), true) | (None, false) => {}
            (None, true) => self.error(
                Span::empty(0),
                "found no `PROTOCOL` header; expected the schema to start with one, such as `PROTOCOL notes VERSION 1`",
            ),
            (Some(header), false) => self.error(
                header.span,
                format!(
                    "found a `PROTOCOL` header in `{path}`; expected it only in `{ROOT_FILE}`, since this file is the scope `{}`; remove it",
                    file.scope.join("/")
                ),
            ),
        }
        if let Some(first) = file.document.definition.first()
            && several
            && file.scope.is_empty()
        {
            self.error(
                first.span(),
                format!(
                    "found a definition in `{ROOT_FILE}`; expected only the `PROTOCOL` header there, since the schema has a file per scope; move it into a scope's file, such as `common.anyx`"
                ),
            );
        }
    }

    fn declare_type(&mut self, name: &ast::Spanned<String>, public: bool) {
        self.check_words(name, "a type");
        if self
            .types
            .iter()
            .any(|known| known.scope == self.scope && known.name == name.value)
        {
            self.error(
                name.span,
                format!(
                    "found a second type named `{}`; expected each type to be defined once in a scope; rename or remove one of them",
                    name.value
                ),
            );
        }
        let declared = self.declared(name, public);
        self.types.push(declared);
    }

    /// Declares the type parameters after `GENERIC`, each a name once, for
    /// the type just declared; none for a type that isn't generic.
    fn declare_type_parameters(&mut self, parameters: Option<&ast::TypeParameters>) {
        let written: Vec<&ast::TypeParameter> = match parameters {
            None => Vec::new(),
            Some(ast::TypeParameters::One(parameter)) => vec![parameter],
            Some(ast::TypeParameters::Several(parameters)) => parameters.value.iter().collect(),
        };
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
        let type_name = self.types[index].name.clone();
        for parameter in self.type_parameters[index].clone() {
            let shadowed: Vec<String> = self
                .types
                .iter()
                .filter(|known| {
                    known.name == parameter.value && (known.scope == self.scope || known.public)
                })
                .map(|known| format!("`{}`", scoped(&known.scope, &known.name)))
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
        let type_name = self.types[index].name.clone();
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
            scope: self.scope.clone(),
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
        if self
            .groups
            .iter()
            .any(|known| known.declared.scope == self.scope && known.declared.name == name.value)
        {
            self.error(
                name.span,
                format!(
                    "found a second error group named `{}`; expected each group once in a scope; rename one of them",
                    name.value
                ),
            );
        }
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
                        "found `{written}` among errors; expected errors in brackets, then groups joined with `+`, such as `[missing (path: TEXT)] + file errors` or `[missing] + files/file errors`"
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
    /// once in a scope.
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
                    .find(|known| known.scope == self.scope && known.name == words)
                {
                    let line = anyx::line_column(&self.sources[other.file], other.span.start).line;
                    self.error(
                        error.name.span,
                        format!(
                            "found a second error `{words}` in {}, after the one on line {line}; expected each error once in a scope; to share one, put it in an `ERRORS` group and join the group with `+`",
                            self.this_scope()
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
                    scope: self.scope.clone(),
                    public,
                    error: shaped,
                });
                found.len() - 1
            })
            .collect()
    }

    /// `reference` as a path: a name in the scope being checked, or a full
    /// path. `None` after reporting it when it is neither.
    fn path(&mut self, reference: &ast::Reference) -> Option<Path> {
        let parts = &reference.name.parts;
        if reference.arguments.is_empty() && !parts.contains(&NamePart::Slot) {
            return Some(Path {
                scope: self.scope.clone(),
                name: spanned_words(&reference.name).value,
                full: false,
            });
        }
        let slashes_between_slots = parts.len() >= 3
            && parts.len() % 2 == 1
            && parts.iter().enumerate().all(|(index, part)| match part {
                NamePart::Slot => index % 2 == 0,
                NamePart::Word { text, .. } => index % 2 == 1 && text == "/",
            });
        let path = reference.arguments.split_last().filter(|(_, scope)| {
            scope
                .iter()
                .all(|segment| matches!(segment.name.parts.as_slice(), [NamePart::Word { .. }]))
        });
        match path {
            Some((name, scope)) if slashes_between_slots => Some(Path {
                scope: scope
                    .iter()
                    .map(|segment| segment.name.text.clone())
                    .collect(),
                name: spanned_words(&name.name).value,
                full: true,
            }),
            _ => {
                let written = self.written(reference.span).to_string();
                self.error(
                    reference.span,
                    format!(
                        "found `{written}`; expected a name, such as `diagnostic`, or the full path of a `PUBLIC` one in another scope, such as `common/diagnostic`"
                    ),
                );
                None
            }
        }
    }

    /// Reports `found` when `path` reaches it from another scope and it is
    /// private.
    fn check_access(&mut self, span: Span, path: &Path, public: bool, what: &str) {
        if public || path.scope == self.scope {
            return;
        }
        self.error(
            span,
            format!(
                "found `{}`, which is private to the scope `{}`; expected a `PUBLIC` {what}, or one of {}; write `PUBLIC` before its definition in `{}` to share it",
                scoped(&path.scope, &path.name),
                path.scope.join("/"),
                self.this_scope(),
                scope_file(&path.scope)
            ),
        );
    }

    /// The full paths of the public declarations named `name` in other
    /// scopes, as a hint after a name that isn't in this scope.
    fn elsewhere<'d>(&self, declared: impl Iterator<Item = &'d Declared>, name: &str) -> String {
        let paths: Vec<String> = declared
            .filter(|known| known.public && known.name == name && known.scope != self.scope)
            .map(|known| format!("`{}`", scoped(&known.scope, &known.name)))
            .collect();
        match paths.is_empty() {
            true => String::new(),
            false => format!(
                "; another scope has {}, so write its full path",
                paths.join(", ")
            ),
        }
    }

    fn resolve_type(&mut self, reference: &ast::Reference) -> Option<usize> {
        let path = self.path(reference)?;
        let found = self
            .types
            .iter()
            .position(|known| known.scope == path.scope && known.name == path.name);
        match found {
            Some(index) => {
                let public = self.types[index].public;
                self.check_access(reference.span, &path, public, "type");
                Some(index)
            }
            None => {
                let message = match path.full {
                    true => format!(
                        "found `{}`, which names no type; expected the full path of a `PUBLIC` `RECORD`, `CHOICE` or `TYPE`, such as `common/diagnostic` for `PUBLIC RECORD diagnostic` in `common.anyx`",
                        scoped(&path.scope, &path.name)
                    ),
                    false => format!(
                        "found `{text}`, which is not a type of {scope}; expected `TEXT`, `BYTES`, `INTEGER`, `NUMBER`, `BOOLEAN`, `INSTANT`, `DURATION`, `LIST: type`, or a `RECORD`, `CHOICE` or `TYPE` defined here; define `{text}`, or fix the name{hint}",
                        text = path.name,
                        scope = self.this_scope(),
                        hint = self.elsewhere(self.types.iter(), &path.name),
                    ),
                };
                self.error(reference.span, message);
                None
            }
        }
    }

    /// The group `reference` names, as an index into the groups.
    fn resolve_group(&mut self, reference: &GroupReference) -> Option<usize> {
        let full = !reference.scope.is_empty();
        let path = Path {
            scope: match full {
                true => reference.scope.clone(),
                false => self.scope.clone(),
            },
            name: reference.name.clone(),
            full,
        };
        let found = self.groups.iter().position(|known| {
            known.declared.scope == path.scope && known.declared.name == path.name
        });
        match found {
            Some(index) => {
                let public = self.groups[index].declared.public;
                self.check_access(reference.span, &path, public, "error group");
                Some(index)
            }
            None => {
                let an_error = self
                    .errors
                    .iter()
                    .any(|known| known.scope == path.scope && known.name == path.name);
                let message = match (path.full, an_error) {
                    (_, true) => format!(
                        "found `{}`, which is an error, not a group; expected a group after a `+`; put the error in an `ERRORS` group and join that group, since an error is defined once, in the brackets of the group or method that has it",
                        scoped(&path.scope, &path.name)
                    ),
                    (true, false) => format!(
                        "found `{}`, which names no error group; expected the full path of a `PUBLIC` `ERRORS` group, such as `files/file errors` for `PUBLIC ERRORS file errors [trashed, missing]` in `files.anyx`",
                        scoped(&path.scope, &path.name)
                    ),
                    (false, false) => format!(
                        "found `{}`, which is not an error group of {}; expected the name of an `ERRORS` group, such as `file errors` for `ERRORS file errors [trashed, missing]`{}",
                        path.name,
                        self.this_scope(),
                        self.elsewhere(self.groups.iter().map(|group| &group.declared), &path.name),
                    ),
                };
                self.error(reference.span, message);
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
        let (file, scope, span, name) = (
            group.declared.file,
            group.declared.scope.clone(),
            group.declared.span,
            group.declared.name.clone(),
        );
        let (own, joined) = (group.own_errors.clone(), group.joined.clone());
        if visiting.contains(&index) {
            self.within(file, &scope, |checker| {
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
        let expanded = self.within(file, &scope, |checker| {
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
            let (r#type, optional) = match &field.r#type {
                ast::FieldType::Optional {
                    r#type: ast::OptionalType::Type(r#type),
                    ..
                } => (self.r#type(r#type), true),
                ast::FieldType::Optional {
                    r#type: ast::OptionalType::Defaulted(defaulted),
                    ..
                } => {
                    let (r#type, node) = self.defaulted(defaulted);
                    if let Some(parameter) = r#type.parameters().first() {
                        self.error(
                            defaulted.span(),
                            format!(
                                "found a `DEFAULT` on the field `{name}`, whose type holds the type parameter `{parameter}`; expected a `DEFAULT` only on a field whose type is the same in every use of `{record}`, since the default can't be checked against each type `{parameter}` stands for",
                                name = field.name.value,
                                record = self.types[record].name,
                            ),
                        );
                    } else {
                        self.defaults.push(PendingDefault {
                            r#type: record,
                            field: checked.len(),
                            node: node.clone(),
                            file: self.file,
                            scope: self.scope.clone(),
                        });
                    }
                    (r#type, true)
                }
                ast::FieldType::RequiredWithDefault(defaulted) => {
                    self.error(
                        defaulted.span(),
                        format!(
                            "found a `DEFAULT` on the field `{name}`, which is not `OPTIONAL`; expected a `DEFAULT` only on an `OPTIONAL` field, which may be left out, as in `{name}: OPTIONAL: INTEGER DEFAULT 50`",
                            name = field.name.value
                        ),
                    );
                    (self.defaulted(defaulted).0, false)
                }
                ast::FieldType::Type(r#type) => (self.r#type(r#type), false),
            };
            checked.push(Field {
                doc: doc(&field.doc),
                name: field.name.value.clone(),
                r#type,
                optional,
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
                        "found a string in {what}'s slot; expected a parameter, as in `(revision: INTEGER)`; {what}'s `#` doc goes on the lines before it"
                    ),
                ),
                ast::VariantSlot::Parameter(parameter) => parameters.push(parameter.clone()),
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
            ..
        } = method_parts(method);
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
            let this = scoped(&declared.scope, &declared.name);
            let same_words = errors
                .iter()
                .map(|known| &self.errors[*known])
                .find(|other| other.name == declared.name)
                .map(|other| scoped(&other.scope, &other.name));
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
                        "found the errors `{other}` and `{this}` both listed for this method; expected errors of distinct words, since a response writes an error by its shape alone, without its scope; rename one of them"
                    ),
                );
            } else {
                errors.push(index);
            }
        }
        Method {
            scope: self.scope.clone(),
            doc: doc(doc_string),
            shape: checked_shape,
            returns: returns.map(|returns| self.parameter(returns)),
            errors,
            examples: Vec::new(),
        }
    }

    fn r#type(&mut self, r#type: &ast::Type) -> Type {
        match r#type {
            ast::Type::Text(_) => Type::Text,
            ast::Type::Bytes(_) => Type::Bytes,
            ast::Type::Integer(_) => Type::Integer,
            ast::Type::Number(_) => Type::Number,
            ast::Type::Boolean(_) => Type::Boolean,
            ast::Type::Instant(_) => Type::Instant,
            ast::Type::Duration(_) => Type::Duration,
            ast::Type::List { item, .. } => Type::List(Box::new(self.r#type(item))),
            ast::Type::Map { value, .. } => Type::Map(Box::new(self.r#type(value))),
            ast::Type::Named(reference) => {
                if let Some(parameter) = self.parameter_named(reference) {
                    return Type::Parameter(parameter);
                }
                let Some(index) = self.resolve_type(reference) else {
                    return Type::Text;
                };
                let parameters = self.parameter_names(index);
                if !parameters.is_empty() {
                    let written = self.written(reference.span).to_string();
                    self.error(
                        reference.span,
                        format!(
                            "found `{written}`, which is `GENERIC ({})`, without its type arguments; expected `{written} WITH ({})`, with a type for each of its parameters",
                            parameters.join(", "),
                            parameters
                                .iter()
                                .map(|parameter| format!("{parameter}: …"))
                                .collect::<Vec<_>>()
                                .join(", "),
                        ),
                    );
                    return Type::Text;
                }
                Type::Named(index)
            }
            ast::Type::Generic {
                reference,
                arguments,
                span,
            } => self.generic_use(reference, arguments, *span),
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

    /// A use of a generic type, `page WITH (item: file summary)`: an
    /// argument for each of its parameters, by name, once each.
    fn generic_use(
        &mut self,
        reference: &ast::Reference,
        arguments: &ast::TypeArguments,
        span: Span,
    ) -> Type {
        if let Some(parameter) = self.parameter_named(reference) {
            self.error(
                span,
                format!(
                    "found the type parameter `{parameter}` with `WITH`; expected `{parameter}` alone, since a type parameter stands for a whole type and takes no arguments"
                ),
            );
            return Type::Parameter(parameter);
        }
        let Some(index) = self.resolve_type(reference) else {
            return Type::Text;
        };
        let written = self.written(reference.span).to_string();
        let parameters = self.parameter_names(index);
        if parameters.is_empty() {
            self.error(
                span,
                format!(
                    "found `WITH` after `{written}`, which is not `GENERIC`; expected `{written}` alone, since only a `GENERIC` record or choice takes type arguments"
                ),
            );
            return Type::Named(index);
        }
        let given: Vec<&ast::Parameter> = match arguments {
            ast::TypeArguments::One(argument) => vec![argument],
            ast::TypeArguments::Several(arguments) => arguments.value.iter().collect(),
        };
        let expected = || {
            parameters
                .iter()
                .map(|parameter| format!("`{parameter}`"))
                .collect::<Vec<_>>()
                .join(", ")
        };
        let mut found: Vec<Option<Type>> = vec![None; parameters.len()];
        for argument in given {
            let name = &argument.name.value;
            let Some(position) = parameters.iter().position(|parameter| parameter == name) else {
                self.error(
                    argument.name.span,
                    format!(
                        "found the type argument `{name}`, which is not a parameter of `{written}`; expected an argument for each of its parameters, {}",
                        expected()
                    ),
                );
                continue;
            };
            let r#type = self.r#type(&argument.r#type);
            if found[position].is_some() {
                self.error(
                    argument.name.span,
                    format!(
                        "found a second type argument for `{name}`; expected each parameter of `{written}` once"
                    ),
                );
                continue;
            }
            found[position] = Some(r#type);
        }
        let missing: Vec<String> = parameters
            .iter()
            .zip(&found)
            .filter(|(_, found)| found.is_none())
            .map(|(parameter, _)| format!("`{parameter}`"))
            .collect();
        if !missing.is_empty() {
            self.error(
                span,
                format!(
                    "found no type argument for {}; expected an argument for each parameter of `{written}`, {}",
                    missing.join(", "),
                    expected()
                ),
            );
        }
        Type::Generic(
            index,
            found
                .into_iter()
                .map(|found| found.unwrap_or(Type::Text))
                .collect(),
        )
    }

    /// The type of a field with a `DEFAULT`, and the default as written.
    fn defaulted<'d>(&mut self, defaulted: &'d ast::Defaulted) -> (Type, &'d anyx::Node) {
        match defaulted {
            ast::Defaulted::Text { default, .. } => (Type::Text, default),
            ast::Defaulted::Bytes { default, .. } => (Type::Bytes, default),
            ast::Defaulted::Integer { default, .. } => (Type::Integer, default),
            ast::Defaulted::Number { default, .. } => (Type::Number, default),
            ast::Defaulted::Boolean { default, .. } => (Type::Boolean, default),
            ast::Defaulted::Instant { default, .. } => (Type::Instant, default),
            ast::Defaulted::Duration { default, .. } => (Type::Duration, default),
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
            ast::Defaulted::Generic {
                reference,
                arguments,
                default,
                span,
            } => (self.generic_use(reference, arguments, *span), default),
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
                Type::List(item) | Type::Map(item) => mentioned(item, found),
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
                    "found the named type `{name}` referring to itself; expected a `TYPE` over a type that doesn't lead back to it, such as `TYPE {name}: TEXT`; to hold values of its own kind, name a `RECORD` or `CHOICE` that holds them through a `LIST`"
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
            let declared = &self.types[start];
            let (file, span, name) = (declared.file, declared.span, declared.name.clone());
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
    /// types, must pass its own parameters as they are: `nest WITH (item:
    /// item)` within `nest`. With other arguments, such as `nest WITH (item:
    /// LIST: item)`, each use would need another, without end.
    fn check_generic_recursion(&mut self, schema: &Schema) {
        /// The generic types `type` uses, anywhere in it, with their
        /// arguments.
        fn uses<'t>(r#type: &'t Type, found: &mut Vec<(usize, &'t [Type])>) {
            match r#type {
                Type::Generic(index, arguments) => {
                    found.push((*index, arguments));
                    arguments.iter().for_each(|argument| uses(argument, found));
                }
                Type::List(item) | Type::Map(item) => uses(item, found),
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
            let declared = &self.types[start];
            let (file, span, name) = (declared.file, declared.span, declared.name.clone());
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
