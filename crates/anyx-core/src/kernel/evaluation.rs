//! Normalization by evaluation for the core kernel.

use super::{Case, Closure, Environment, Index, Level, Neutral, Pattern, Term, Value};
use crate::lowering::GlobalKind;
use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EvaluationError {
    UnboundIndex(Index),
    UnboundLevel(Level),
    ApplyNonFunction,
    NoMatchingCase,
}

impl fmt::Display for EvaluationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnboundIndex(Index(index)) => {
                write!(formatter, "local index {index} is out of scope")
            }
            Self::UnboundLevel(Level(level)) => {
                write!(formatter, "local level {level} is out of scope")
            }
            Self::ApplyNonFunction => write!(formatter, "cannot apply a non-function value"),
            Self::NoMatchingCase => write!(formatter, "no case matches the subject"),
        }
    }
}

impl std::error::Error for EvaluationError {}

/// Evaluate a term with one environment entry per enclosing binder.
pub fn evaluate(term: &Term, environment: &[Value]) -> Result<Value, EvaluationError> {
    Ok(match term {
        Term::Type => Value::Type,
        Term::Local(index) => {
            let position = reverse_position(environment.len(), index.0)
                .ok_or(EvaluationError::UnboundIndex(*index))?;
            environment[position].clone()
        }
        Term::Global(global) if global.kind == GlobalKind::Constructor => Value::Constructor {
            global: global.clone(),
            arguments: Vec::new(),
        },
        Term::Global(global) => Value::Neutral(Neutral::Global(global.clone())),
        Term::FunctionType {
            parameter_type,
            result,
        } => Value::FunctionType {
            parameter_type: Box::new(evaluate(parameter_type, environment)?),
            result: Closure {
                environment: environment.to_vec(),
                body: result.clone(),
            },
        },
        Term::Function { body } => Value::Function(Closure {
            environment: environment.to_vec(),
            body: body.clone(),
        }),
        Term::Apply {
            function,
            arguments,
        } => {
            let mut function = evaluate(function, environment)?;
            for argument in arguments {
                function = apply(function, evaluate(argument, environment)?)?;
            }
            function
        }
        Term::Match { subject, cases } => {
            evaluate_match(evaluate(subject, environment)?, cases, environment)?
        }
        Term::List(elements) => Value::List(
            elements
                .iter()
                .map(|element| evaluate(element, environment))
                .collect::<Result<_, _>>()?,
        ),
        Term::Text(text) => Value::Text(text.clone()),
        Term::Number(number) => Value::Number(*number),
    })
}

impl Closure {
    /// Evaluate the body with its parameter bound to the supplied value.
    pub fn instantiate(&self, argument: Value) -> Result<Value, EvaluationError> {
        let mut environment = self.environment.clone();
        environment.push(argument);
        evaluate(&self.body, &environment)
    }
}

fn apply(function: Value, argument: Value) -> Result<Value, EvaluationError> {
    match function {
        Value::Function(closure) => closure.instantiate(argument),
        Value::Constructor {
            global,
            mut arguments,
        } => {
            arguments.push(argument);
            Ok(Value::Constructor { global, arguments })
        }
        Value::Neutral(Neutral::Apply {
            function,
            mut arguments,
        }) => {
            arguments.push(argument);
            Ok(Value::Neutral(Neutral::Apply {
                function,
                arguments,
            }))
        }
        Value::Neutral(function) => Ok(Value::Neutral(Neutral::Apply {
            function: Box::new(function),
            arguments: vec![argument],
        })),
        _ => Err(EvaluationError::ApplyNonFunction),
    }
}

enum PatternMatch {
    Matched,
    Mismatched,
    Blocked,
}

fn evaluate_match(
    subject: Value,
    cases: &[Case],
    environment: &[Value],
) -> Result<Value, EvaluationError> {
    if cases.is_empty() && matches!(subject, Value::Neutral(_)) {
        return Ok(Value::Neutral(Neutral::Match {
            subject: Box::new(subject),
            cases: Vec::new(),
            environment: Environment::new(),
        }));
    }

    for (case_index, case) in cases.iter().enumerate() {
        let mut bindings = Vec::new();
        match match_pattern(&case.pattern, &subject, &mut bindings) {
            PatternMatch::Matched => {
                let mut environment = environment.to_vec();
                environment.extend(bindings);
                return evaluate(&case.result, &environment);
            }
            PatternMatch::Mismatched => {}
            PatternMatch::Blocked => {
                return Ok(Value::Neutral(Neutral::Match {
                    subject: Box::new(subject),
                    cases: cases[case_index..].to_vec(),
                    environment: environment.to_vec(),
                }));
            }
        }
    }
    Err(EvaluationError::NoMatchingCase)
}

fn match_pattern(pattern: &Pattern, subject: &Value, bindings: &mut Environment) -> PatternMatch {
    let Pattern::Constructor {
        name,
        arguments: patterns,
    } = pattern
    else {
        bindings.push(subject.clone());
        return PatternMatch::Matched;
    };
    match subject {
        Value::Constructor { global, arguments }
            if name == &global.name && patterns.len() == arguments.len() =>
        {
            let mut result = PatternMatch::Matched;
            for (pattern, argument) in patterns.iter().zip(arguments) {
                match match_pattern(pattern, argument, bindings) {
                    PatternMatch::Matched => {}
                    PatternMatch::Mismatched => return PatternMatch::Mismatched,
                    PatternMatch::Blocked => result = PatternMatch::Blocked,
                }
            }
            result
        }
        Value::Neutral(_) => PatternMatch::Blocked,
        _ => PatternMatch::Mismatched,
    }
}

/// Reify a value under the given number of enclosing binders.
pub fn read_back(value: &Value, depth: usize) -> Result<Term, EvaluationError> {
    Ok(match value {
        Value::Type => Term::Type,
        Value::FunctionType {
            parameter_type,
            result,
        } => Term::FunctionType {
            parameter_type: Box::new(read_back(parameter_type, depth)?),
            result: read_back_closure(result, depth)?,
        },
        Value::Function(closure) => Term::Function {
            body: read_back_closure(closure, depth)?,
        },
        Value::Constructor { global, arguments } => {
            read_back_application(Term::Global(global.clone()), arguments, depth)?
        }
        Value::Neutral(neutral) => read_back_neutral(neutral, depth)?,
        Value::List(elements) => Term::List(read_back_values(elements, depth)?),
        Value::Text(text) => Term::Text(text.clone()),
        Value::Number(number) => Term::Number(*number),
    })
}

fn read_back_closure(closure: &Closure, depth: usize) -> Result<Box<Term>, EvaluationError> {
    let value = closure.instantiate(fresh_local(depth))?;
    read_back(&value, depth + 1).map(Box::new)
}

fn read_back_neutral(neutral: &Neutral, depth: usize) -> Result<Term, EvaluationError> {
    Ok(match neutral {
        Neutral::Local(level) => Term::Local(Index(
            reverse_position(depth, level.0).ok_or(EvaluationError::UnboundLevel(*level))?,
        )),
        Neutral::Global(global) => Term::Global(global.clone()),
        Neutral::Apply {
            function,
            arguments,
        } => read_back_application(read_back_neutral(function, depth)?, arguments, depth)?,
        Neutral::Match {
            subject,
            cases,
            environment,
        } => Term::Match {
            subject: Box::new(read_back(subject, depth)?),
            cases: cases
                .iter()
                .map(|case| read_back_case(case, environment, depth))
                .collect::<Result<_, _>>()?,
        },
    })
}

fn read_back_case(
    case: &Case,
    environment: &[Value],
    depth: usize,
) -> Result<Case, EvaluationError> {
    let mut environment = environment.to_vec();
    let mut result_depth = depth;
    bind_pattern_locals(&case.pattern, &mut environment, &mut result_depth);
    Ok(Case {
        pattern: case.pattern.clone(),
        result: read_back(&evaluate(&case.result, &environment)?, result_depth)?,
    })
}

fn bind_pattern_locals(pattern: &Pattern, environment: &mut Environment, depth: &mut usize) {
    match pattern {
        Pattern::Binding => {
            environment.push(fresh_local(*depth));
            *depth += 1;
        }
        Pattern::Constructor { arguments, .. } => {
            for argument in arguments {
                bind_pattern_locals(argument, environment, depth);
            }
        }
    }
}

fn read_back_application(
    function: Term,
    arguments: &[Value],
    depth: usize,
) -> Result<Term, EvaluationError> {
    if arguments.is_empty() {
        return Ok(function);
    }
    Ok(Term::Apply {
        function: Box::new(function),
        arguments: read_back_values(arguments, depth)?,
    })
}

fn read_back_values(values: &[Value], depth: usize) -> Result<Vec<Term>, EvaluationError> {
    values.iter().map(|value| read_back(value, depth)).collect()
}

pub(super) fn reverse_position(length: usize, position: usize) -> Option<usize> {
    length.checked_sub(1)?.checked_sub(position)
}

pub(super) fn fresh_local(depth: usize) -> Value {
    Value::Neutral(Neutral::Local(Level(depth)))
}

/// Normalize an open term with the given number of enclosing binders.
pub fn normalize(term: &Term, depth: usize) -> Result<Term, EvaluationError> {
    let environment: Environment = (0..depth).map(fresh_local).collect();
    read_back(&evaluate(term, &environment)?, depth)
}

/// Compare beta-normal forms in the same local scope.
pub fn convertible(left: &Term, right: &Term, depth: usize) -> Result<bool, EvaluationError> {
    Ok(normalize(left, depth)? == normalize(right, depth)?)
}
