//! Name resolution and lowering from the generated syntax tree.

use crate::{kernel, parser};
use anyx::Span;
use std::{collections::HashMap, fmt};

#[derive(Debug, PartialEq)]
pub struct Document {
    pub declarations: Vec<Declaration>,
}

#[derive(Debug, PartialEq)]
pub enum Declaration {
    Data {
        signature: Signature,
        constructors: Vec<Signature>,
        /// Written as a `RECORD`: one constructor, whose parameters are the
        /// record's parameters followed by its fields' binders.
        record: bool,
    },
    Function {
        signature: Signature,
        body: Term,
    },
    /// A function without a body: whatever serves the schema implements it.
    Served {
        signature: Signature,
    },
    /// One slot of a record field's `DEFAULT`, checked as a function without
    /// parameters and called where a `{ }` value leaves the field out.
    Default {
        signature: Signature,
        body: Term,
    },
}

#[derive(Debug, PartialEq)]
pub struct Signature {
    pub name: String,
    pub parameters: Vec<Parameter>,
    pub result: Term,
}

#[derive(Debug, PartialEq)]
pub struct Parameter {
    pub binding: Binding,
    pub annotation: Box<Term>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BindingIdentifier(usize);

#[derive(Debug, PartialEq)]
pub struct Binding {
    pub identifier: BindingIdentifier,
    /// Anonymous functions bind slots without introducing a source name.
    pub name: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GlobalKind {
    Data,
    Function,
    Constructor,
}

#[derive(Debug, PartialEq)]
pub enum Term {
    Type,
    Kind,
    Local(BindingIdentifier),
    Global {
        name: String,
        kind: GlobalKind,
    },
    FunctionType {
        parameter: Parameter,
        result: Box<Term>,
    },
    Function {
        parameter: Binding,
        body: Box<Term>,
    },
    Apply {
        function: Box<Term>,
        arguments: Vec<Term>,
    },
    Match {
        subject: Box<Term>,
        cases: Vec<Case>,
    },
    List(Vec<Term>),
    /// `3`, a literal of the built-in `Natural`, or of `Integer` where one
    /// is expected.
    Natural(u128),
    /// `-3`, a literal of the built-in `Integer`.
    Integer(i128),
    /// `"src"`, a literal of the built-in `Text`.
    Text(String),
}

#[derive(Debug, PartialEq)]
pub struct Case {
    pub pattern: Pattern,
    pub result: Term,
}

#[derive(Debug, PartialEq)]
pub enum Pattern {
    Constructor {
        name: String,
        arguments: Vec<Pattern>,
    },
    Binding(Binding),
}

#[derive(Debug, PartialEq, Eq)]
pub struct LoweringError {
    pub span: Span,
    pub message: String,
}

impl fmt::Display for LoweringError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for LoweringError {}

/// Resolve a single file. All its declaration heads are visible throughout it.
pub fn lower_document(document: &parser::Document) -> Result<Document, LoweringError> {
    let mut resolver = Resolver::default();
    resolver.register_document(document)?;

    let mut declarations = Vec::new();
    for declaration in &document.declaration {
        declarations.push(match declaration {
            parser::Declaration::Data(data) => {
                let signature =
                    resolver.declaration_signature(&data.head, &data.signature_result)?;
                resolver.locals.clear();
                let mut constructors = Vec::new();
                for constructor in &data.constructors.value {
                    constructors
                        .push(resolver.signature(&constructor.head, Some(&constructor.r#type))?);
                    resolver.locals.clear();
                }
                Declaration::Data {
                    signature,
                    constructors,
                    record: false,
                }
            }
            parser::Declaration::Record(record) => {
                let (data, defaults) = resolver.record(record)?;
                declarations.push(data);
                declarations.extend(defaults);
                continue;
            }
            parser::Declaration::Function(function) => {
                let signature =
                    resolver.declaration_signature(&function.head, &function.signature_result)?;
                let body = resolver.term(&function.body)?;
                resolver.locals.clear();
                Declaration::Function { signature, body }
            }
            parser::Declaration::ServedFunction(function) => {
                let signature = resolver.signature(&function.head, Some(&function.result))?;
                resolver.locals.clear();
                Declaration::Served { signature }
            }
        });
    }
    Ok(Document { declarations })
}

/// Resolve a closed expression in the namespace of a parsed document.
pub fn lower_expression(
    document: &parser::Document,
    expression: &parser::Expression,
) -> Result<Term, LoweringError> {
    let mut resolver = Resolver::default();
    resolver.register_document(document)?;
    resolver.term(expression)
}

pub(crate) fn head_parts(head: &parser::Head) -> (&parser::CallName, &[parser::Parameter]) {
    match head {
        parser::Head::Plain { name, .. } => (name, &[]),
        parser::Head::WithParameters {
            name, arguments, ..
        } => (name, arguments),
    }
}

#[derive(Default)]
struct Resolver<'source> {
    globals: HashMap<&'source str, GlobalKind>,
    locals: Vec<(&'source str, BindingIdentifier)>,
    next_binding: usize,
    /// The data type of each constructor, to find an unnamed field.
    constructor_data: HashMap<&'source str, &'source str>,
    records: Vec<RecordShape>,
}

/// What a record entry is known by.
enum Key<'entry> {
    /// The words of a field, or a constructor of an unnamed field's type.
    Name(&'entry str),
    /// The record a nested `{ }` value is.
    Data(&'entry str),
}

/// What a `{ }` value needs to know about a record.
#[derive(Clone)]
struct RecordShape {
    name: String,
    constructor: String,
    has_parameters: bool,
    fields: Vec<FieldShape>,
}

#[derive(Clone)]
struct FieldShape {
    /// The field's words with its slots, `between _ and _`; `None` for a
    /// field written as its value alone.
    name: Option<String>,
    /// The data type of a field written as its value alone.
    unnamed_type: Option<String>,
    binders: Vec<String>,
    has_default: bool,
}

/// The name of a record's constructor: its words, then a slot for each of
/// its parameters and binders. `File summary` with four fields is
/// `file summary _ _ _ _`.
pub fn record_constructor_name(record: &str, slots: usize) -> String {
    let words: Vec<&str> = record
        .split_whitespace()
        .filter(|word| *word != "_")
        .collect();
    let mut name = words.join(" ").to_lowercase();
    for _ in 0..slots {
        name.push_str(" _");
    }
    name
}

/// The function that holds one slot of a field's `DEFAULT`.
pub fn default_name(record: &str, binder: &str) -> String {
    format!(
        "default {binder} of {}",
        record
            .replace('_', "")
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    )
}

fn record_slots(record: &parser::Record) -> usize {
    let (_, parameters) = head_parts(&record.head);
    parameters.len()
        + record
            .fields
            .value
            .iter()
            .map(|field| field_parts(field).1.len())
            .sum::<usize>()
}

/// A field's words (none for a field written as its value alone), its
/// slots, and where it is written.
pub(crate) fn field_parts(
    field: &parser::Field,
) -> (Option<&parser::CallName>, &[parser::Parameter], Span) {
    let shape = match field {
        parser::Field::Defaulted { field_shape, .. } | parser::Field::FieldShape(field_shape) => {
            field_shape
        }
    };
    match shape {
        parser::FieldShape::Unnamed(parameter) => {
            (None, std::slice::from_ref(parameter), parameter.span)
        }
        parser::FieldShape::Named(head) => {
            let (name, slots) = head_parts(head);
            (Some(name), slots, head.span())
        }
    }
}

fn shape_of(record: &parser::Record) -> RecordShape {
    let (name, parameters) = head_parts(&record.head);
    RecordShape {
        name: name.text.clone(),
        constructor: record_constructor_name(&name.text, record_slots(record)),
        has_parameters: !parameters.is_empty(),
        fields: record
            .fields
            .value
            .iter()
            .map(|field| {
                let (field_name, slots, _) = field_parts(field);
                let unnamed = field_name.is_none();
                FieldShape {
                    name: field_name.map(|name| name.text.clone()),
                    unnamed_type: match slots.first().map(|slot| &*slot.r#type) {
                        Some(parser::Expression::Name { name, .. }) if unnamed => {
                            Some(name.text.clone())
                        }
                        _ => None,
                    },
                    binders: slots
                        .iter()
                        .map(|slot| slot.binder.name.value.clone())
                        .collect(),
                    has_default: matches!(field, parser::Field::Defaulted { .. }),
                }
            })
            .collect(),
    }
}

impl<'source> Resolver<'source> {
    fn register_document(
        &mut self,
        document: &'source parser::Document,
    ) -> Result<(), LoweringError> {
        // The kernel's built-in types, which it checks before the file.
        for data in kernel::BUILT_IN {
            self.globals.insert(data, GlobalKind::Data);
        }
        for (constructor, data) in [
            (kernel::ZERO, kernel::NATURAL),
            (kernel::SUCCESSOR, kernel::NATURAL),
            (kernel::EMPTY_LIST, kernel::LIST),
            (kernel::LIST_CELL, kernel::LIST),
        ] {
            self.globals.insert(constructor, GlobalKind::Constructor);
            self.constructor_data.insert(constructor, data);
        }
        for declaration in &document.declaration {
            match declaration {
                parser::Declaration::Data(data) => {
                    self.register(&data.head, GlobalKind::Data)?;
                    let (data_name, _) = head_parts(&data.head);
                    for constructor in &data.constructors.value {
                        self.register(&constructor.head, GlobalKind::Constructor)?;
                        let (name, _) = head_parts(&constructor.head);
                        self.constructor_data.insert(&name.text, &data_name.text);
                    }
                }
                parser::Declaration::Record(record) => {
                    self.register(&record.head, GlobalKind::Data)?;
                    let (name, _) = head_parts(&record.head);
                    let constructor = record_constructor_name(&name.text, record_slots(record));
                    let constructor =
                        self.register_generated(&constructor, name.span, GlobalKind::Constructor)?;
                    // A record is a `DATA`, so it can be an unnamed field.
                    self.constructor_data.insert(constructor, &name.text);
                    self.records.push(shape_of(record));
                    for field in &record.fields.value {
                        if let parser::Field::Defaulted { .. } = field {
                            for parameter in field_parts(field).1 {
                                let default =
                                    default_name(&name.text, &parameter.binder.name.value);
                                self.register_generated(&default, name.span, GlobalKind::Function)?;
                            }
                        }
                    }
                }
                parser::Declaration::Function(function) => {
                    self.register(&function.head, GlobalKind::Function)?;
                }
                parser::Declaration::ServedFunction(function) => {
                    self.register(&function.head, GlobalKind::Function)?;
                }
            }
        }
        Ok(())
    }

    fn register(
        &mut self,
        head: &'source parser::Head,
        kind: GlobalKind,
    ) -> Result<(), LoweringError> {
        let (name, _) = head_parts(head);
        check_binder_name(&name.text, name.span)?;
        if self.globals.insert(&name.text, kind).is_some() {
            return Err(LoweringError {
                span: name.span,
                message: format!("duplicate declaration of `{}`", name.text),
            });
        }
        Ok(())
    }

    /// A global the source names only through a record.
    fn register_generated(
        &mut self,
        name: &str,
        span: Span,
        kind: GlobalKind,
    ) -> Result<&'source str, LoweringError> {
        let name: &'source str = Box::leak(name.to_owned().into_boxed_str());
        if self.globals.insert(name, kind).is_some() {
            return Err(LoweringError {
                span,
                message: format!("duplicate declaration of `{name}`"),
            });
        }
        Ok(name)
    }

    /// A record as a data type with one constructor, and a function for each
    /// slot of each `DEFAULT`.
    fn record(
        &mut self,
        record: &'source parser::Record,
    ) -> Result<(Declaration, Vec<Declaration>), LoweringError> {
        let (name, record_parameters) = head_parts(&record.head);
        let signature = self.declaration_signature(&record.head, &record.signature_result)?;
        self.locals.clear();

        // The constructor: the record's parameters, then every field's slots.
        let mut parameters = Vec::new();
        for parameter in record_parameters {
            parameters.push(self.parameter(parameter, 0)?);
        }
        let mut shape = RecordShape {
            name: name.text.clone(),
            constructor: record_constructor_name(&name.text, record_slots(record)),
            has_parameters: !record_parameters.is_empty(),
            fields: Vec::new(),
        };
        for field in &record.fields.value {
            let (field_name, slots, field_span) = field_parts(field);
            let unnamed_type = match field_name {
                None => Some(self.unnamed_type(&slots[0], &shape)?),
                Some(field_name) if slots.is_empty() => {
                    return Err(LoweringError {
                        span: field_span,
                        message: format!(
                            "the field `{0}` holds nothing: give it a slot, as in `{0} (name: Type)`",
                            field_name.text
                        ),
                    });
                }
                Some(field_name) => {
                    // An entry is found by a named field's words first, so
                    // they mustn't also be a value of an unnamed field.
                    let data = self.constructor_data.get(field_name.text.as_str());
                    let names_an_unnamed_value = record.fields.value.iter().any(|other| {
                        let (other_name, other_slots, _) = field_parts(other);
                        other_name.is_none()
                            && matches!(
                                (&*other_slots[0].r#type, data),
                                (parser::Expression::Name { name, .. }, Some(data))
                                    if name.text == *data
                            )
                    });
                    if names_an_unnamed_value
                        || shape
                            .fields
                            .iter()
                            .any(|other| other.name.as_deref() == Some(&field_name.text))
                    {
                        return Err(LoweringError {
                            span: field_name.span,
                            message: format!(
                                "`{}` already names another field or a value of one, so an entry couldn't tell which it fills",
                                field_name.text
                            ),
                        });
                    }
                    None
                }
            };
            for slot in slots {
                parameters.push(self.parameter(slot, 0)?);
            }
            shape.fields.push(FieldShape {
                name: field_name.map(|name| name.text.clone()),
                unnamed_type,
                binders: slots
                    .iter()
                    .map(|slot| slot.binder.name.value.clone())
                    .collect(),
                has_default: matches!(field, parser::Field::Defaulted { .. }),
            });
        }
        let record_type = Term::Global {
            name: name.text.clone(),
            kind: GlobalKind::Data,
        };
        let result = if record_parameters.is_empty() {
            record_type
        } else {
            Term::Apply {
                function: Box::new(record_type),
                arguments: parameters[..record_parameters.len()]
                    .iter()
                    .map(|parameter| Term::Local(parameter.binding.identifier))
                    .collect(),
            }
        };
        let constructor = Signature {
            name: shape.constructor.clone(),
            parameters,
            result,
        };
        self.locals.clear();

        // Each default slot is checked on its own, so it may name no field.
        let mut defaults = Vec::new();
        for field in &record.fields.value {
            let parser::Field::Defaulted { entry, .. } = field else {
                continue;
            };
            let (field_name, slots, _) = field_parts(field);
            let values: Vec<&parser::Expression> = match field_name {
                None => vec![entry],
                Some(field_name) => match entry {
                    parser::Expression::Call {
                        name: entry_name,
                        arguments,
                        ..
                    } if entry_name.text == field_name.text => arguments.iter().collect(),
                    _ => {
                        return Err(LoweringError {
                            span: entry.span(),
                            message: format!(
                                "a `DEFAULT` is the field as written: `{}` with a value in each slot",
                                field_name.text
                            ),
                        });
                    }
                },
            };
            for (slot, value) in slots.iter().zip(values) {
                let result = self.term(&slot.r#type).map_err(|error| {
                    let binders = || {
                        record
                            .fields
                            .value
                            .iter()
                            .flat_map(|field| field_parts(field).1)
                            .map(|slot| slot.binder.name.value.as_str())
                    };
                    match binders()
                        .find(|binder| error.message == format!("unknown name `{binder}`"))
                    {
                        Some(binder) => LoweringError {
                            span: entry.span(),
                            message: format!(
                                "`{}` depends on the field `{binder}`, so it has no `DEFAULT`",
                                slot.binder.name.value
                            ),
                        },
                        None => error,
                    }
                })?;
                let body = self.term(value)?;
                defaults.push(Declaration::Default {
                    signature: Signature {
                        name: default_name(&name.text, &slot.binder.name.value),
                        parameters: Vec::new(),
                        result,
                    },
                    body,
                });
            }
        }
        Ok((
            Declaration::Data {
                signature,
                constructors: vec![constructor],
                record: true,
            },
            defaults,
        ))
    }

    /// The data type a field written as its value alone holds: an entry finds
    /// the field by its constructor.
    fn unnamed_type(
        &self,
        slot: &parser::Parameter,
        shape: &RecordShape,
    ) -> Result<String, LoweringError> {
        let parser::Expression::Name { name, .. } = &*slot.r#type else {
            return Err(LoweringError {
                span: slot.r#type.span(),
                message:
                    "a field written as its value alone holds a `DATA` named without arguments"
                        .to_owned(),
            });
        };
        if !self
            .constructor_data
            .values()
            .any(|data| *data == name.text)
        {
            return Err(LoweringError {
                span: name.span,
                message: format!(
                    "a field written as its value alone holds a `DATA` with constructors; `{}` isn't one",
                    name.text
                ),
            });
        }
        if shape
            .fields
            .iter()
            .any(|field| field.unnamed_type.as_deref() == Some(&name.text))
        {
            return Err(LoweringError {
                span: name.span,
                message: format!(
                    "two fields written as their value alone both hold `{}`, so an entry couldn't tell which it fills",
                    name.text
                ),
            });
        }
        Ok(name.text.clone())
    }

    /// `{ entries }`: the one record whose fields the entries fill, built in
    /// its fields' order, with each left-out field's `DEFAULT`.
    fn record_value(
        &mut self,
        span: Span,
        entries: &'source [parser::Expression],
    ) -> Result<Term, LoweringError> {
        let (record, filled) = self.fitting_record(span, entries)?;
        if self.records[record].has_parameters {
            return Err(LoweringError {
                span,
                message: format!(
                    "`{}` has parameters, and a `{{ }}` value can't give them yet",
                    self.records[record].name
                ),
            });
        }
        let shape = self.records[record].clone();
        let mut arguments = Vec::new();
        for (field_index, field) in shape.fields.iter().enumerate() {
            match filled.iter().position(|filled| *filled == field_index) {
                Some(entry_index) => match (&entries[entry_index], &field.name) {
                    (
                        parser::Expression::Call {
                            arguments: values, ..
                        },
                        Some(_),
                    ) => {
                        for value in values {
                            arguments.push(self.term(value)?);
                        }
                    }
                    (entry, _) => arguments.push(self.term(entry)?),
                },
                None => {
                    for binder in &field.binders {
                        arguments.push(Term::Global {
                            name: default_name(&shape.name, binder),
                            kind: GlobalKind::Function,
                        });
                    }
                }
            }
        }
        let constructor = Term::Global {
            name: shape.constructor.clone(),
            kind: GlobalKind::Constructor,
        };
        Ok(if arguments.is_empty() {
            constructor
        } else {
            Term::Apply {
                function: Box::new(constructor),
                arguments,
            }
        })
    }

    /// The one record whose fields `entries` fill, and the field each entry
    /// fills. An entry is a field's words, as in `between 1 and 3`, or a
    /// value whose data type an unnamed field holds: a constructor call, or
    /// a `{ }` value of another record.
    fn fitting_record(
        &self,
        span: Span,
        entries: &[parser::Expression],
    ) -> Result<(usize, Vec<usize>), LoweringError> {
        let mut keys = Vec::new();
        for entry in entries {
            keys.push(match entry {
                parser::Expression::Call { name, .. } | parser::Expression::Name { name, .. } => {
                    Key::Name(name.text.as_str())
                }
                parser::Expression::RecordValue(nested) => {
                    let (record, _) = self.fitting_record(entry.span(), &nested.value)?;
                    Key::Data(self.records[record].name.as_str())
                }
                _ => {
                    return Err(LoweringError {
                        span: entry.span(),
                        message: "a record entry is a field as written, such as `between 1 and 3`"
                            .to_owned(),
                    });
                }
            });
        }
        let fits: Vec<(usize, Vec<usize>)> = self
            .records
            .iter()
            .enumerate()
            .filter_map(|(index, record)| self.fill(record, &keys).map(|filled| (index, filled)))
            .collect();
        match fits.as_slice() {
            [(record, filled)] => Ok((*record, filled.clone())),
            [] => Err(LoweringError {
                span,
                message: "no record has exactly these fields".to_owned(),
            }),
            _ => {
                let names: Vec<String> = fits
                    .iter()
                    .map(|(record, _)| format!("`{}`", self.records[*record].name))
                    .collect();
                Err(LoweringError {
                    span,
                    message: format!(
                        "these entries fit more than one record: {}",
                        names.join(", ")
                    ),
                })
            }
        }
    }

    fn fill(&self, record: &RecordShape, keys: &[Key]) -> Option<Vec<usize>> {
        let mut filled = Vec::new();
        for key in keys {
            let (name, data) = match key {
                Key::Name(name) => (Some(*name), self.constructor_data.get(name).copied()),
                Key::Data(data) => (None, Some(*data)),
            };
            let field = record.fields.iter().position(|field| {
                (name.is_some() && field.name.as_deref() == name)
                    || (field.unnamed_type.is_some() && field.unnamed_type.as_deref() == data)
            })?;
            if filled.contains(&field) {
                return None;
            }
            filled.push(field);
        }
        record
            .fields
            .iter()
            .enumerate()
            .all(|(index, field)| field.has_default || filled.contains(&index))
            .then_some(filled)
    }

    fn declaration_signature(
        &mut self,
        head: &'source parser::Head,
        result: &'source parser::SignatureResult,
    ) -> Result<Signature, LoweringError> {
        self.signature(
            head,
            match result {
                parser::SignatureResult::Kind(_) => None,
                parser::SignatureResult::Expression(expression) => Some(expression),
            },
        )
    }

    fn signature(
        &mut self,
        head: &'source parser::Head,
        result: Option<&'source parser::Expression>,
    ) -> Result<Signature, LoweringError> {
        let (name, parameters) = head_parts(head);
        let mut lowered_parameters = Vec::new();
        for parameter in parameters {
            lowered_parameters.push(self.parameter(parameter, 0)?);
        }
        Ok(Signature {
            name: name.text.clone(),
            parameters: lowered_parameters,
            result: match result {
                Some(expression) => self.term(expression)?,
                None => Term::Kind,
            },
        })
    }

    fn new_binding(&mut self, name: Option<&'source str>) -> Binding {
        let identifier = BindingIdentifier(self.next_binding);
        self.next_binding += 1;
        if let Some(name) = name {
            self.locals.push((name, identifier));
        }
        Binding {
            identifier,
            name: name.map(str::to_owned),
        }
    }

    fn named_binding(
        &mut self,
        name: &'source str,
        span: Span,
        scope_start: usize,
    ) -> Result<Binding, LoweringError> {
        check_binder_name(name, span)?;
        if self.locals[scope_start..]
            .iter()
            .any(|(local, _)| *local == name)
        {
            return Err(LoweringError {
                span,
                message: format!("duplicate binding of `{name}`"),
            });
        }
        Ok(self.new_binding(Some(name)))
    }

    fn parameter(
        &mut self,
        parameter: &'source parser::Parameter,
        scope_start: usize,
    ) -> Result<Parameter, LoweringError> {
        let annotation = Box::new(self.term(&parameter.r#type)?);
        let name = &parameter.binder.name;
        let binding = self.named_binding(&name.value, name.span, scope_start)?;
        Ok(Parameter {
            binding,
            annotation,
        })
    }

    fn reference(&self, name: &parser::CallName) -> Result<Term, LoweringError> {
        if name.text == "Type" {
            return Ok(Term::Type);
        }
        if name.text == "Kind" {
            return Err(LoweringError {
                span: name.span,
                message: "`Kind` is only allowed as a declaration result".to_owned(),
            });
        }
        if let Some((_, identifier)) = self
            .locals
            .iter()
            .rev()
            .find(|(local, _)| *local == name.text)
        {
            return Ok(Term::Local(*identifier));
        }
        if let Some(kind) = self.globals.get(name.text.as_str()) {
            return Ok(Term::Global {
                name: name.text.clone(),
                kind: *kind,
            });
        }
        Err(LoweringError {
            span: name.span,
            message: format!("unknown name `{}`", name.text),
        })
    }

    fn term(&mut self, expression: &'source parser::Expression) -> Result<Term, LoweringError> {
        Ok(match expression {
            parser::Expression::FunctionType {
                parameter, result, ..
            } => {
                let scope_start = self.locals.len();
                let parameter = self.parameter(parameter, scope_start)?;
                let result = Box::new(self.term(result)?);
                self.locals.truncate(scope_start);
                Term::FunctionType { parameter, result }
            }
            parser::Expression::Match { subject, cases, .. } => Term::Match {
                subject: Box::new(self.term(subject)?),
                cases: cases
                    .value
                    .iter()
                    .map(|case| self.case(case))
                    .collect::<Result<_, _>>()?,
            },
            parser::Expression::Call {
                name, arguments, ..
            } => self.call(name, arguments)?,
            parser::Expression::Name { name, .. } => self.reference(name)?,
            parser::Expression::Hole(span) => {
                return Err(LoweringError {
                    span: *span,
                    message: "a hole must be a direct argument of a call".to_owned(),
                });
            }
            parser::Expression::Number(number) => {
                let too_large = || LoweringError {
                    span: number.span,
                    message: format!("`{}` is too large for a number", number.value),
                };
                if number.value.starts_with('-') {
                    Term::Integer(number.value.parse().map_err(|_| too_large())?)
                } else {
                    Term::Natural(number.value.parse().map_err(|_| too_large())?)
                }
            }
            parser::Expression::Text(text) => Term::Text(text.value.value.clone()),
            parser::Expression::RecordValue(entries) => {
                self.record_value(entries.span, &entries.value)?
            }
            parser::Expression::List(elements) => Term::List(
                elements
                    .value
                    .iter()
                    .map(|element| self.term(element))
                    .collect::<Result<_, _>>()?,
            ),
        })
    }

    fn call(
        &mut self,
        name: &parser::CallName,
        arguments: &'source [parser::Expression],
    ) -> Result<Term, LoweringError> {
        let function = Box::new(self.reference(name)?);
        let mut parameters = Vec::new();
        let mut lowered_arguments = Vec::new();
        for argument in arguments {
            lowered_arguments.push(match argument {
                parser::Expression::Hole(_) => {
                    let parameter = self.new_binding(None);
                    let reference = Term::Local(parameter.identifier);
                    parameters.push(parameter);
                    reference
                }
                _ => self.term(argument)?,
            });
        }
        let mut term = Term::Apply {
            function,
            arguments: lowered_arguments,
        };
        for parameter in parameters.into_iter().rev() {
            term = Term::Function {
                parameter,
                body: Box::new(term),
            };
        }
        Ok(term)
    }

    fn case(&mut self, case: &'source parser::Case) -> Result<Case, LoweringError> {
        let scope_start = self.locals.len();
        let pattern = self.pattern(&case.pattern, scope_start)?;
        let result = self.term(&case.result)?;
        self.locals.truncate(scope_start);
        Ok(Case { pattern, result })
    }

    fn pattern(
        &mut self,
        pattern: &'source parser::Pattern,
        scope_start: usize,
    ) -> Result<Pattern, LoweringError> {
        let (name, arguments) = match pattern {
            parser::Pattern::Name { name, .. }
                if self.globals.get(name.text.as_str()) != Some(&GlobalKind::Constructor) =>
            {
                return Ok(Pattern::Binding(self.named_binding(
                    &name.text,
                    name.span,
                    scope_start,
                )?));
            }
            parser::Pattern::Name { name, .. } => (name, &[][..]),
            parser::Pattern::Constructor {
                name, arguments, ..
            } => (name, arguments.as_slice()),
        };
        if self.globals.get(name.text.as_str()) != Some(&GlobalKind::Constructor) {
            return Err(LoweringError {
                span: name.span,
                message: format!("unknown constructor `{}`", name.text),
            });
        }
        Ok(Pattern::Constructor {
            name: name.text.clone(),
            arguments: arguments
                .iter()
                .map(|argument| self.pattern(argument, scope_start))
                .collect::<Result<_, _>>()?,
        })
    }
}

fn check_binder_name(name: &str, span: Span) -> Result<(), LoweringError> {
    if matches!(name, "Type" | "Kind") {
        return Err(LoweringError {
            span,
            message: format!("`{name}` is reserved"),
        });
    }
    Ok(())
}
