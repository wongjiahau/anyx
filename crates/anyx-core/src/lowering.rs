//! Name resolution and lowering from the generated syntax tree.

use crate::parser;
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
    },
    Function {
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
                }
            }
            parser::Declaration::Function(function) => {
                let signature =
                    resolver.declaration_signature(&function.head, &function.signature_result)?;
                let body = resolver.term(&function.body)?;
                resolver.locals.clear();
                Declaration::Function { signature, body }
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
}

impl<'source> Resolver<'source> {
    fn register_document(
        &mut self,
        document: &'source parser::Document,
    ) -> Result<(), LoweringError> {
        for declaration in &document.declaration {
            match declaration {
                parser::Declaration::Data(data) => {
                    self.register(&data.head, GlobalKind::Data)?;
                    for constructor in &data.constructors.value {
                        self.register(&constructor.head, GlobalKind::Constructor)?;
                    }
                }
                parser::Declaration::Function(function) => {
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
        let binding =
            self.named_binding(&parameter.name.value, parameter.name.span, scope_start)?;
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
