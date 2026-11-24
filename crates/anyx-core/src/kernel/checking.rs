//! Bidirectional checking for core expressions.

#[path = "index_unification.rs"]
mod index_unification;
#[path = "match_checking.rs"]
mod match_checking;
use match_checking::DataFamilies;

use super::data_checking::check_data_declarations;
use super::evaluation::{Definitions, fresh_local, reverse_position};
use super::termination::check_termination;
use super::{
    Declaration, Document, Environment, EvaluationError, Global, Index, Signature, Term, Value,
};
use crate::lowering::GlobalKind;
use std::collections::HashMap;
use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CheckingError {
    UnboundLocal(Index),
    UnknownGlobal(Global),
    FunctionNeedsExpectedType,
    ExpectedFunctionType,
    TypeMismatch,
    KindHasNoType,
    ConstructorFieldMustBeInType,
    UnsupportedTerm,
    MatchNeedsExpectedType,
    MatchNeedsData,
    ImpossiblePattern(String),
    WrongPatternConstructor(String),
    WrongPatternArgumentCount {
        constructor: String,
        expected: usize,
        actual: usize,
    },
    RedundantCase(usize),
    NonExhaustiveMatch {
        pattern: String,
    },
    Evaluation(EvaluationError),
}

impl fmt::Display for CheckingError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnboundLocal(Index(index)) => {
                write!(formatter, "local index {index} is out of scope")
            }
            Self::UnknownGlobal(global) => {
                write!(formatter, "no type supplied for global `{}`", global.name)
            }
            Self::FunctionNeedsExpectedType => write!(
                formatter,
                "a function made from holes needs an expected function type"
            ),
            Self::ExpectedFunctionType => write!(formatter, "expected a function type"),
            Self::TypeMismatch => write!(formatter, "expression does not have the expected type"),
            Self::KindHasNoType => write!(formatter, "Kind has no type"),
            Self::ConstructorFieldMustBeInType => write!(
                formatter,
                "a constructor field of data returning Type must be in Type"
            ),
            Self::UnsupportedTerm => write!(formatter, "checking literals is not yet supported"),
            Self::MatchNeedsExpectedType => {
                write!(formatter, "MATCH needs an expected result type")
            }
            Self::MatchNeedsData => write!(
                formatter,
                "MATCH and constructor patterns need a fully applied declared data type"
            ),
            Self::ImpossiblePattern(name) => {
                write!(
                    formatter,
                    "constructor pattern `{name}` has incompatible indices"
                )
            }
            Self::WrongPatternConstructor(name) => write!(
                formatter,
                "constructor `{name}` does not belong to the pattern's data type"
            ),
            Self::WrongPatternArgumentCount {
                constructor,
                expected,
                actual,
            } => write!(
                formatter,
                "constructor pattern `{constructor}` has {actual} arguments, expected {expected}"
            ),
            Self::RedundantCase(position) => {
                write!(formatter, "MATCH case {position} is redundant")
            }
            Self::NonExhaustiveMatch { pattern } => {
                write!(
                    formatter,
                    "MATCH is not exhaustive (missing case: {pattern})"
                )
            }
            Self::Evaluation(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for CheckingError {}

impl From<EvaluationError> for CheckingError {
    fn from(error: EvaluationError) -> Self {
        Self::Evaluation(error)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DeclarationCheckingError {
    DuplicateGlobal(String),
    NonTerminatingFunctions(Vec<String>),
    Signature { name: String, error: CheckingError },
    Body { name: String, error: CheckingError },
    DataMustReturnUniverse(String),
    ConstructorMustReturnData { constructor: String, data: String },
    NonPositiveOccurrence { declaration: String, data: String },
}

impl fmt::Display for DeclarationCheckingError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonTerminatingFunctions(names) => write!(
                formatter,
                "cannot establish structural termination for `{}`",
                names.join("`, `")
            ),
            Self::DuplicateGlobal(name) => write!(formatter, "duplicate global `{name}`"),
            Self::Signature { name, error } => {
                write!(formatter, "signature of `{name}`: {error}")
            }
            Self::Body { name, error } => write!(formatter, "body of `{name}`: {error}"),
            Self::DataMustReturnUniverse(name) => {
                write!(
                    formatter,
                    "data declaration `{name}` must return Type or Kind"
                )
            }
            Self::ConstructorMustReturnData { constructor, data } => {
                write!(
                    formatter,
                    "constructor `{constructor}` must return a fully applied `{data}`"
                )
            }
            Self::NonPositiveOccurrence { declaration, data } => {
                write!(
                    formatter,
                    "recursive occurrence of `{data}` in `{declaration}` is not strictly positive"
                )
            }
        }
    }
}

impl std::error::Error for DeclarationCheckingError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Signature { error, .. } | Self::Body { error, .. } => Some(error),
            _ => None,
        }
    }
}

pub type FunctionCheckingError = DeclarationCheckingError;

/// Checks core terms using closed global types.
#[derive(Clone, Default)]
pub struct TypeChecker {
    globals: HashMap<String, (Global, Term)>,
    data_families: DataFamilies,
    definitions: Definitions,
}

impl TypeChecker {
    /// Evaluate a supplied global type in the empty scope and register it by name.
    pub fn with_global_type(
        mut self,
        global: Global,
        global_type: &Term,
    ) -> Result<Self, CheckingError> {
        self.definitions.evaluate(global_type, &[])?;
        self.globals
            .insert(global.name.clone(), (global, global_type.clone()));
        Ok(self)
    }

    /// Check every function in a document and make its signature available for
    /// subsequent expression checking. Data and constructor types can be supplied
    /// with `with_global_type`.
    pub fn check_functions(self, document: &Document) -> Result<Self, FunctionCheckingError> {
        self.check_declarations(
            document
                .declarations
                .iter()
                .filter(|declaration| matches!(declaration, Declaration::Function { .. })),
        )
    }

    /// Check all declarations, including data families and their constructors.
    pub fn check_document(self, document: &Document) -> Result<Self, DeclarationCheckingError> {
        self.check_declarations(document.declarations.iter())
    }

    fn check_declarations<'document>(
        mut self,
        declarations: impl Iterator<Item = &'document Declaration> + Clone,
    ) -> Result<Self, DeclarationCheckingError> {
        check_data_declarations(declarations.clone())?;

        let mut signatures = Vec::new();
        let mut functions = Vec::new();
        for declaration in declarations.clone() {
            match declaration {
                Declaration::Function { signature, body } => {
                    signatures.push((signature, GlobalKind::Function, false));
                    functions.push((signature, body));
                }
                Declaration::Data {
                    signature,
                    constructors,
                } => {
                    signatures.push((signature, GlobalKind::Data, false));
                    self.data_families.register(signature, constructors);
                    signatures.extend(constructors.iter().map(|constructor| {
                        (
                            constructor,
                            GlobalKind::Constructor,
                            signature.result == Term::Type,
                        )
                    }));
                }
            }
        }
        for (signature, kind, _) in &signatures {
            if self.globals.contains_key(&signature.name) {
                return Err(DeclarationCheckingError::DuplicateGlobal(
                    signature.name.clone(),
                ));
            }
            self = self
                .with_global_type(
                    Global {
                        name: signature.name.clone(),
                        kind: *kind,
                    },
                    &closed_function_type(signature),
                )
                .map_err(|error| DeclarationCheckingError::Signature {
                    name: signature.name.clone(),
                    error,
                })?;
        }

        let scope = self.scope();
        for (signature, _, fields_must_be_in_type) in &signatures {
            scope
                .check_signature(signature, *fields_must_be_in_type)
                .map_err(|error| DeclarationCheckingError::Signature {
                    name: signature.name.clone(),
                    error,
                })?;
        }

        for group in check_termination(&functions)? {
            let scope = self.scope();
            for (signature, body) in &group.functions {
                let mut closed_body = (*body).clone();
                for _ in &signature.parameter_types {
                    closed_body = Term::Function {
                        body: Box::new(closed_body),
                    };
                }
                let (_, expected) = &self.globals[&signature.name];
                let check_body = || {
                    let expected = self.definitions.evaluate(expected, &[])?;
                    scope.check_value(&closed_body, &expected)
                };
                check_body().map_err(|error| DeclarationCheckingError::Body {
                    name: signature.name.clone(),
                    error,
                })?;
            }
            for (signature, body) in &group.functions {
                self.definitions
                    .0
                    .insert(signature.name.clone(), group.definition(signature, body));
            }
        }

        Ok(self)
    }

    /// Check that a closed annotation belongs to a universe.
    pub fn check_type(&self, annotation: &Term) -> Result<(), CheckingError> {
        self.scope().type_universe(annotation).map(|_| ())
    }

    /// Check a closed expression against a closed type.
    pub fn check(&self, term: &Term, expected: &Term) -> Result<(), CheckingError> {
        let scope = self.scope();
        scope.type_universe(expected)?;
        scope.check_value(term, &self.definitions.evaluate(expected, &[])?)
    }

    /// Synthesize the type of a closed expression from explicit type information.
    pub fn synthesize(&self, term: &Term) -> Result<Term, CheckingError> {
        Ok(self
            .definitions
            .read_back(&self.scope().synthesize_value(term)?, 0)?)
    }

    /// Normalize a closed, checked term using the checked document's definitions.
    pub fn normalize(&self, term: &Term) -> Result<Term, EvaluationError> {
        let value = self.definitions.evaluate(term, &[])?;
        self.definitions.read_back(&value, 0)
    }

    fn scope(&self) -> Scope<'_> {
        Scope {
            globals: &self.globals,
            definitions: &self.definitions,
            data_families: &self.data_families,
            local_types: Vec::new(),
            environment: Vec::new(),
            refinements: Vec::new(),
            blocked_indices: Vec::new(),
        }
    }
}

fn closed_function_type(signature: &Signature) -> Term {
    let mut result = signature.result.clone();
    for parameter_type in signature.parameter_types.iter().rev() {
        result = Term::FunctionType {
            parameter_type: Box::new(parameter_type.clone()),
            result: Box::new(result),
        };
    }
    result
}

#[derive(Clone)]
struct Scope<'globals> {
    globals: &'globals HashMap<String, (Global, Term)>,
    definitions: &'globals Definitions,
    data_families: &'globals DataFamilies,
    local_types: Vec<Value>,
    environment: Environment,
    // Semantic levels include constructor fields that have no source binder.
    refinements: Environment,
    blocked_indices: Vec<(Value, Value)>,
}

impl Scope<'_> {
    fn under_parameter(&self, parameter_type: Value) -> (Self, Value) {
        let mut scope = self.clone();
        let parameter = scope.push_parameter(parameter_type);
        (scope, parameter)
    }

    fn push_parameter(&mut self, parameter_type: Value) -> Value {
        let parameter = self.fresh_value();
        self.bind_parameter(parameter_type, parameter.clone());
        parameter
    }

    fn fresh_value(&mut self) -> Value {
        let value = fresh_local(self.refinements.len());
        self.refinements.push(value.clone());
        value
    }

    fn bind_parameter(&mut self, parameter_type: Value, value: Value) {
        self.local_types.push(parameter_type);
        self.environment.push(value);
    }

    fn check_signature(
        &self,
        signature: &Signature,
        fields_must_be_in_type: bool,
    ) -> Result<(), CheckingError> {
        let mut scope = self.clone();
        for parameter_type in &signature.parameter_types {
            let universe = scope.type_universe(parameter_type)?;
            if fields_must_be_in_type && matches!(universe, Value::Kind) {
                return Err(CheckingError::ConstructorFieldMustBeInType);
            }
            let parameter_type = scope
                .definitions
                .evaluate(parameter_type, &scope.environment)?;
            scope.push_parameter(parameter_type);
        }
        if signature.result != Term::Kind {
            scope.type_universe(&signature.result)?;
        }
        Ok(())
    }

    fn type_universe(&self, term: &Term) -> Result<Value, CheckingError> {
        if let Term::Match { subject, cases } = term {
            // An inline type-valued match has the same fixed universe as before.
            self.check_match(subject, cases, &Value::Type)?;
            return Ok(Value::Type);
        }
        match self.synthesize_value(term)? {
            universe @ (Value::Type | Value::Kind) => Ok(universe),
            _ => Err(CheckingError::TypeMismatch),
        }
    }

    fn check_value(&self, term: &Term, expected: &Value) -> Result<(), CheckingError> {
        let expected = self.refine(expected)?;
        if let Term::Function { body } = term {
            let Value::FunctionType {
                parameter_type,
                result,
            } = &expected
            else {
                return Err(CheckingError::ExpectedFunctionType);
            };
            let (scope, parameter) = self.under_parameter(parameter_type.as_ref().clone());
            return scope.check_value(body, &self.definitions.instantiate(result, parameter)?);
        }

        if let Term::Match { subject, cases } = term {
            return self.check_match(subject, cases, &expected);
        }

        let actual = self.synthesize_value(term)?;
        let depth = self.refinements.len();
        if self.definitions.read_back(&actual, depth)?
            == self.definitions.read_back(&expected, depth)?
        {
            Ok(())
        } else {
            Err(CheckingError::TypeMismatch)
        }
    }

    fn synthesize_value(&self, term: &Term) -> Result<Value, CheckingError> {
        match term {
            Term::Type => Ok(Value::Kind),
            Term::Kind => Err(CheckingError::KindHasNoType),
            Term::Local(index) => {
                let position = reverse_position(self.local_types.len(), index.0)
                    .ok_or(CheckingError::UnboundLocal(*index))?;
                self.refine(&self.local_types[position])
            }
            Term::Global(global) => {
                let Some((registered, global_type)) = self.globals.get(&global.name) else {
                    return Err(CheckingError::UnknownGlobal(global.clone()));
                };
                if registered != global {
                    return Err(CheckingError::UnknownGlobal(global.clone()));
                }
                Ok(self.definitions.evaluate(global_type, &[])?)
            }
            Term::FunctionType {
                parameter_type,
                result,
            } => {
                let parameter_universe = self.type_universe(parameter_type)?;
                let (scope, _) = self.under_parameter(
                    self.definitions
                        .evaluate(parameter_type, &self.environment)?,
                );
                let result_universe = scope.type_universe(result)?;
                if matches!(parameter_universe, Value::Kind) {
                    Ok(Value::Kind)
                } else {
                    Ok(result_universe)
                }
            }
            Term::Function { .. } => Err(CheckingError::FunctionNeedsExpectedType),
            Term::Apply {
                function,
                arguments,
            } => {
                let mut function_type = self.synthesize_value(function)?;
                for argument in arguments {
                    let Value::FunctionType {
                        parameter_type,
                        result,
                    } = function_type
                    else {
                        return Err(CheckingError::ExpectedFunctionType);
                    };
                    self.check_value(argument, &parameter_type)?;
                    function_type = self.definitions.instantiate(
                        &result,
                        self.definitions.evaluate(argument, &self.environment)?,
                    )?;
                }
                Ok(function_type)
            }
            Term::Match { .. } => Err(CheckingError::MatchNeedsExpectedType),
            Term::List(_) => Err(CheckingError::UnsupportedTerm),
        }
    }
}
