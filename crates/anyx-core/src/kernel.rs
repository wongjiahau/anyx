//! Kernel terms and the values used to evaluate them.

use crate::lowering::{self, BindingIdentifier, GlobalKind};
use std::fmt;

mod checking;
pub use checking::{CheckingError, DeclarationCheckingError, FunctionCheckingError, TypeChecker};
mod data_checking;

/// For each constructor of a data family, the positions of its binders that
/// stand for the family's parameters rather than stored fields or indices.
pub fn parameter_binders(
    signature: &Signature,
    constructors: &[Signature],
) -> Vec<std::collections::HashSet<usize>> {
    data_checking::parameter_binders(signature, constructors)
}
mod evaluation;
mod termination;
pub use evaluation::{EvaluationError, convertible, evaluate, normalize, read_back};

/// Distance from the innermost binder, which has index zero.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Index(pub usize);

/// Position from the outermost binder, which has level zero.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Level(pub usize);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Global {
    pub name: String,
    pub kind: GlobalKind,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Document {
    pub declarations: Vec<Declaration>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Declaration {
    Data {
        signature: Signature,
        constructors: Vec<Signature>,
    },
    Function {
        signature: Signature,
        body: Term,
    },
    /// A function without a body, an opaque constant for checking.
    Served {
        signature: Signature,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Signature {
    pub name: String,
    /// Each annotation is in scope of the preceding parameters.
    pub parameter_types: Vec<Term>,
    /// The result and the declaration body are in scope of all parameters.
    pub result: Term,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Term {
    Type,
    Kind,
    Local(Index),
    Global(Global),
    FunctionType {
        parameter_type: Box<Term>,
        result: Box<Term>,
    },
    Function {
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

#[derive(Clone, Debug, PartialEq)]
pub struct Case {
    pub pattern: Pattern,
    /// Pattern binders enter scope in left-to-right traversal order.
    pub result: Term,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Pattern {
    Constructor {
        name: String,
        arguments: Vec<Pattern>,
    },
    Binding,
}

#[derive(Clone, Debug)]
pub enum Value {
    Type,
    Kind,
    FunctionType {
        parameter_type: Box<Value>,
        result: Closure,
    },
    Function(Closure),
    Constructor {
        global: Global,
        arguments: Vec<Value>,
    },
    Neutral(Neutral),
    List(Vec<Value>),
}

#[derive(Clone, Debug)]
pub struct Closure {
    pub environment: Environment,
    /// The closure's parameter is index zero in this term.
    pub body: Box<Term>,
}

/// Captured values in outermost-to-innermost order.
pub type Environment = Vec<Value>;

/// A reference or elimination whose evaluation is blocked.
#[derive(Clone, Debug)]
pub enum Neutral {
    Local(Level),
    Global(Global),
    Apply {
        function: Box<Neutral>,
        arguments: Vec<Value>,
    },
    Match {
        subject: Box<Value>,
        cases: Vec<Case>,
        environment: Environment,
    },
}

/// A local reference to a binding that is absent from the current scope.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UnboundLocal(pub BindingIdentifier);

impl fmt::Display for UnboundLocal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "local binding {:?} is out of scope", self.0)
    }
}

impl std::error::Error for UnboundLocal {}

impl TryFrom<&lowering::Document> for Document {
    type Error = UnboundLocal;

    fn try_from(document: &lowering::Document) -> Result<Self, Self::Error> {
        let mut declarations = Vec::new();
        for declaration in &document.declarations {
            let mut scope = Scope::default();
            declarations.push(match declaration {
                lowering::Declaration::Data {
                    signature,
                    constructors,
                } => Declaration::Data {
                    signature: scope.signature(signature)?,
                    constructors: constructors
                        .iter()
                        .map(|constructor| Scope::default().signature(constructor))
                        .collect::<Result<_, _>>()?,
                },
                lowering::Declaration::Function { signature, body } => Declaration::Function {
                    signature: scope.signature(signature)?,
                    body: scope.term(body)?,
                },
                lowering::Declaration::Served { signature } => Declaration::Served {
                    signature: scope.signature(signature)?,
                },
            });
        }
        Ok(Self { declarations })
    }
}

impl TryFrom<&lowering::Term> for Term {
    type Error = UnboundLocal;

    fn try_from(term: &lowering::Term) -> Result<Self, Self::Error> {
        Scope::default().term(term)
    }
}

#[derive(Default)]
struct Scope {
    bindings: Vec<BindingIdentifier>,
}

impl Scope {
    fn signature(&mut self, signature: &lowering::Signature) -> Result<Signature, UnboundLocal> {
        let mut parameter_types = Vec::new();
        for parameter in &signature.parameters {
            parameter_types.push(self.term(&parameter.annotation)?);
            self.bindings.push(parameter.binding.identifier);
        }
        Ok(Signature {
            name: signature.name.clone(),
            parameter_types,
            result: self.term(&signature.result)?,
        })
    }

    fn term(&mut self, term: &lowering::Term) -> Result<Term, UnboundLocal> {
        Ok(match term {
            lowering::Term::Type => Term::Type,
            lowering::Term::Kind => Term::Kind,
            lowering::Term::Local(binding) => Term::Local(
                self.bindings
                    .iter()
                    .rev()
                    .position(|in_scope| in_scope == binding)
                    .map(Index)
                    .ok_or(UnboundLocal(*binding))?,
            ),
            lowering::Term::Global { name, kind } => Term::Global(Global {
                name: name.clone(),
                kind: *kind,
            }),
            lowering::Term::FunctionType { parameter, result } => Term::FunctionType {
                parameter_type: Box::new(self.term(&parameter.annotation)?),
                result: self.under_binding(parameter.binding.identifier, result)?,
            },
            lowering::Term::Function { parameter, body } => Term::Function {
                body: self.under_binding(parameter.identifier, body)?,
            },
            lowering::Term::Apply {
                function,
                arguments,
            } => Term::Apply {
                function: Box::new(self.term(function)?),
                arguments: self.terms(arguments)?,
            },
            lowering::Term::Match { subject, cases } => Term::Match {
                subject: Box::new(self.term(subject)?),
                cases: cases
                    .iter()
                    .map(|case| self.case(case))
                    .collect::<Result<_, _>>()?,
            },
            lowering::Term::List(elements) => Term::List(self.terms(elements)?),
        })
    }

    fn terms(&mut self, terms: &[lowering::Term]) -> Result<Vec<Term>, UnboundLocal> {
        terms.iter().map(|term| self.term(term)).collect()
    }

    fn under_binding(
        &mut self,
        binding: BindingIdentifier,
        term: &lowering::Term,
    ) -> Result<Box<Term>, UnboundLocal> {
        self.bindings.push(binding);
        let result = self.term(term).map(Box::new);
        self.bindings.pop();
        result
    }

    fn case(&mut self, case: &lowering::Case) -> Result<Case, UnboundLocal> {
        let scope_start = self.bindings.len();
        let pattern = self.pattern(&case.pattern);
        let result = self.term(&case.result);
        self.bindings.truncate(scope_start);
        Ok(Case {
            pattern,
            result: result?,
        })
    }

    fn pattern(&mut self, pattern: &lowering::Pattern) -> Pattern {
        match pattern {
            lowering::Pattern::Constructor { name, arguments } => Pattern::Constructor {
                name: name.clone(),
                arguments: arguments
                    .iter()
                    .map(|argument| self.pattern(argument))
                    .collect(),
            },
            lowering::Pattern::Binding(binding) => {
                self.bindings.push(binding.identifier);
                Pattern::Binding
            }
        }
    }
}
