//! Bidirectional checking for core expressions.

use super::evaluation::{fresh_local, reverse_position};
use super::{Environment, EvaluationError, Global, Index, Term, Value, evaluate, read_back};
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

/// Checks core terms using caller-supplied, well-formed closed global types.
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
