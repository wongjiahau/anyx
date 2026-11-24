//! Bidirectional checking for core expressions.

use super::evaluation::{fresh_local, reverse_position};
use super::{
    Declaration, Document, Environment, EvaluationError, Global, Index, Signature, Term, Value,
    evaluate, read_back,
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
    UnsupportedTerm,
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
            Self::UnsupportedTerm => write!(
                formatter,
                "checking matches and literals is not yet supported"
            ),
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
pub enum FunctionCheckingError {
    DuplicateGlobal(String),
    Signature { name: String, error: CheckingError },
    Body { name: String, error: CheckingError },
}

impl fmt::Display for FunctionCheckingError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateGlobal(name) => write!(formatter, "duplicate global `{name}`"),
            Self::Signature { name, error } => {
                write!(formatter, "signature of `{name}`: {error}")
            }
            Self::Body { name, error } => write!(formatter, "body of `{name}`: {error}"),
        }
    }
}

impl std::error::Error for FunctionCheckingError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Signature { error, .. } | Self::Body { error, .. } => Some(error),
            Self::DuplicateGlobal(_) => None,
        }
    }
}

/// Checks core terms using closed global types.
#[derive(Clone, Default)]
pub struct TypeChecker {
    globals: HashMap<String, (Global, Value)>,
}

impl TypeChecker {
    /// Evaluate a supplied global type in the empty scope and register it by name.
    pub fn with_global_type(
        mut self,
        global: Global,
        global_type: &Term,
    ) -> Result<Self, CheckingError> {
        let global_type = evaluate(global_type, &[])?;
        self.globals
            .insert(global.name.clone(), (global, global_type));
        Ok(self)
    }

    /// Check every function in a document and make its signature available for
    /// subsequent expression checking. Data and constructor types can be supplied
    /// with `with_global_type`.
    pub fn check_functions(mut self, document: &Document) -> Result<Self, FunctionCheckingError> {
        let functions: Vec<_> = document
            .declarations
            .iter()
            .filter_map(|declaration| match declaration {
                Declaration::Function { signature, body } => {
                    Some((signature, closed_function_type(signature), body))
                }
                Declaration::Data { .. } => None,
            })
            .collect();

        for (signature, function_type, _) in &functions {
            if self.globals.contains_key(&signature.name) {
                return Err(FunctionCheckingError::DuplicateGlobal(
                    signature.name.clone(),
                ));
            }
            self = self
                .with_global_type(
                    Global {
                        name: signature.name.clone(),
                        kind: GlobalKind::Function,
                    },
                    function_type,
                )
                .map_err(|error| FunctionCheckingError::Signature {
                    name: signature.name.clone(),
                    error,
                })?;
        }

        let scope = self.scope();
        for (signature, function_type, _) in &functions {
            scope
                .check_value(function_type, &Value::Type)
                .map_err(|error| FunctionCheckingError::Signature {
                    name: signature.name.clone(),
                    error,
                })?;
        }

        for (signature, _, body) in functions {
            let mut closed_body = body.clone();
            for _ in &signature.parameter_types {
                closed_body = Term::Function {
                    body: Box::new(closed_body),
                };
            }
            let (_, expected) = &self.globals[&signature.name];
            scope.check_value(&closed_body, expected).map_err(|error| {
                FunctionCheckingError::Body {
                    name: signature.name.clone(),
                    error,
                }
            })?;
        }

        Ok(self)
    }

    /// Check a closed expression against a closed type.
    pub fn check(&self, term: &Term, expected: &Term) -> Result<(), CheckingError> {
        let scope = self.scope();
        scope.check_value(expected, &Value::Type)?;
        scope.check_value(term, &evaluate(expected, &[])?)
    }

    /// Synthesize the type of a closed expression from explicit type information.
    pub fn synthesize(&self, term: &Term) -> Result<Term, CheckingError> {
        Ok(read_back(&self.scope().synthesize_value(term)?, 0)?)
    }

    fn scope(&self) -> Scope<'_> {
        Scope {
            globals: &self.globals,
            local_types: Vec::new(),
            environment: Vec::new(),
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
    globals: &'globals HashMap<String, (Global, Value)>,
    local_types: Vec<Value>,
    environment: Environment,
}

impl Scope<'_> {
    fn under_parameter(&self, parameter_type: Value) -> (Self, Value) {
        let mut scope = self.clone();
        let parameter = fresh_local(self.environment.len());
        scope.local_types.push(parameter_type);
        scope.environment.push(parameter.clone());
        (scope, parameter)
    }

    fn check_value(&self, term: &Term, expected: &Value) -> Result<(), CheckingError> {
        if let Term::Function { body } = term {
            let Value::FunctionType {
                parameter_type,
                result,
            } = expected
            else {
                return Err(CheckingError::ExpectedFunctionType);
            };
            let (scope, parameter) = self.under_parameter(parameter_type.as_ref().clone());
            return scope.check_value(body, &result.instantiate(parameter)?);
        }

        let actual = self.synthesize_value(term)?;
        let depth = self.environment.len();
        if read_back(&actual, depth)? == read_back(expected, depth)? {
            Ok(())
        } else {
            Err(CheckingError::TypeMismatch)
        }
    }

    fn synthesize_value(&self, term: &Term) -> Result<Value, CheckingError> {
        match term {
            Term::Type => Ok(Value::Type),
            Term::Local(index) => {
                let position = reverse_position(self.local_types.len(), index.0)
                    .ok_or(CheckingError::UnboundLocal(*index))?;
                Ok(self.local_types[position].clone())
            }
            Term::Global(global) => {
                let Some((registered, global_type)) = self.globals.get(&global.name) else {
                    return Err(CheckingError::UnknownGlobal(global.clone()));
                };
                if registered != global {
                    return Err(CheckingError::UnknownGlobal(global.clone()));
                }
                Ok(global_type.clone())
            }
            Term::FunctionType {
                parameter_type,
                result,
            } => {
                self.check_value(parameter_type, &Value::Type)?;
                let (scope, _) = self.under_parameter(evaluate(parameter_type, &self.environment)?);
                scope.check_value(result, &Value::Type)?;
                Ok(Value::Type)
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
                    function_type = result.instantiate(evaluate(argument, &self.environment)?)?;
                }
                Ok(function_type)
            }
            Term::Match { .. } | Term::List(_) | Term::Text(_) | Term::Number(_) => {
                Err(CheckingError::UnsupportedTerm)
            }
        }
    }
}
