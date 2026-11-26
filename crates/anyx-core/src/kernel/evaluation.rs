//! Normalization by evaluation for the core kernel.

use super::{
    Case, Closure, Environment, Global, Index, Level, Neutral, Pattern, SUCCESSOR, Term, Value,
    ZERO,
};
use crate::lowering::GlobalKind;
use std::collections::HashMap;
use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EvaluationError {
    UnboundIndex(Index),
    UnboundLevel(Level),
    ApplyNonFunction,
    NoMatchingCase,
    /// A `Natural` past the largest the kernel holds, 2^128 - 1.
    NaturalOverflow,
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
            Self::NaturalOverflow => write!(formatter, "a `Natural` grew past 2^128 - 1"),
        }
    }
}

impl std::error::Error for EvaluationError {}

/// Evaluate a term with one environment entry per enclosing binder.
pub fn evaluate(term: &Term, environment: &[Value]) -> Result<Value, EvaluationError> {
    Definitions::default().evaluate(term, environment)
}

#[derive(Clone, Debug)]
pub(super) struct Definition {
    pub body: Term,
    pub decreasing_parameter: Option<usize>,
    pub parameter_count: usize,
}

#[derive(Clone, Default)]
pub(super) struct Definitions(pub HashMap<String, Definition>);

impl Definitions {
    pub fn evaluate(&self, term: &Term, environment: &[Value]) -> Result<Value, EvaluationError> {
        Ok(match term {
            Term::Type => Value::Type,
            Term::Kind => Value::Kind,
            Term::Local(index) => {
                let position = reverse_position(environment.len(), index.0)
                    .ok_or(EvaluationError::UnboundIndex(*index))?;
                environment[position].clone()
            }
            Term::Global(global)
                if global.kind == GlobalKind::Constructor && global.name == ZERO =>
            {
                Value::Natural(0)
            }
            Term::Global(global) if global.kind == GlobalKind::Constructor => Value::Constructor {
                global: global.clone(),
                arguments: Vec::new(),
            },
            Term::Natural(number) => Value::Natural(*number),
            Term::Integer(number) => Value::Integer(*number),
            Term::Text(text) => Value::Text(text.clone()),
            Term::Global(global) => self.evaluate_global(global, Vec::new())?,
            Term::FunctionType {
                parameter_type,
                result,
            } => Value::FunctionType {
                parameter_type: Box::new(self.evaluate(parameter_type, environment)?),
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
                let mut function = self.evaluate(function, environment)?;
                for argument in arguments {
                    function = self.apply(function, self.evaluate(argument, environment)?)?;
                }
                function
            }
            Term::Match { subject, cases } => {
                self.evaluate_match(self.evaluate(subject, environment)?, cases, environment)?
            }
            Term::List(elements) => Value::List(
                elements
                    .iter()
                    .map(|element| self.evaluate(element, environment))
                    .collect::<Result<_, _>>()?,
            ),
        })
    }

    pub fn instantiate(
        &self,
        closure: &Closure,
        argument: Value,
    ) -> Result<Value, EvaluationError> {
        let mut environment = closure.environment.clone();
        environment.push(argument);
        self.evaluate(&closure.body, &environment)
    }

    fn evaluate_global(
        &self,
        global: &Global,
        arguments: Vec<Value>,
    ) -> Result<Value, EvaluationError> {
        if global.kind == GlobalKind::Function
            && let Some(definition) = self.0.get(&global.name)
        {
            if let Some(position) = definition.decreasing_parameter {
                if arguments.len() == definition.parameter_count
                    && matches!(
                        arguments[position],
                        Value::Constructor { .. } | Value::Natural(_)
                    )
                {
                    return self.evaluate(&definition.body, &arguments);
                }
            } else {
                return self.evaluate(&definition.body, &[]);
            }
        }

        let function = Neutral::Global(global.clone());
        Ok(Value::Neutral(if arguments.is_empty() {
            function
        } else {
            Neutral::Apply {
                function: Box::new(function),
                arguments,
            }
        }))
    }

    fn apply(&self, function: Value, argument: Value) -> Result<Value, EvaluationError> {
        match function {
            Value::Function(closure) => self.instantiate(&closure, argument),
            Value::Constructor {
                global,
                mut arguments,
            } => {
                // `successor of` a number is the next number.
                if global.name == SUCCESSOR
                    && arguments.is_empty()
                    && let Value::Natural(number) = argument
                {
                    return number
                        .checked_add(1)
                        .map(Value::Natural)
                        .ok_or(EvaluationError::NaturalOverflow);
                }
                arguments.push(argument);
                Ok(Value::Constructor { global, arguments })
            }
            Value::Neutral(Neutral::Apply {
                function,
                mut arguments,
            }) => {
                arguments.push(argument);
                if let Neutral::Global(global) = function.as_ref() {
                    return self.evaluate_global(global, arguments);
                }
                Ok(Value::Neutral(Neutral::Apply {
                    function,
                    arguments,
                }))
            }
            Value::Neutral(Neutral::Global(global)) => {
                self.evaluate_global(&global, vec![argument])
            }
            Value::Neutral(function) => Ok(Value::Neutral(Neutral::Apply {
                function: Box::new(function),
                arguments: vec![argument],
            })),
            _ => Err(EvaluationError::ApplyNonFunction),
        }
    }

    fn evaluate_match(
        &self,
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
                    return self.evaluate(&case.result, &environment);
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
}

impl Closure {
    /// Evaluate the body with its parameter bound to the supplied value.
    pub fn instantiate(&self, argument: Value) -> Result<Value, EvaluationError> {
        let mut environment = self.environment.clone();
        environment.push(argument);
        evaluate(&self.body, &environment)
    }
}

enum PatternMatch {
    Matched,
    Mismatched,
    Blocked,
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
    match subject.peeled().as_ref() {
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
        // A `[ ]` literal doesn't say its item type, which the list
        // constructors take, so it isn't taken apart: the match waits.
        Value::List(_) => PatternMatch::Blocked,
        _ => PatternMatch::Mismatched,
    }
}

/// Reify a value under the given number of enclosing binders.
pub fn read_back(value: &Value, depth: usize) -> Result<Term, EvaluationError> {
    Definitions::default().read_back(value, depth)
}

impl Definitions {
    pub fn read_back(&self, value: &Value, depth: usize) -> Result<Term, EvaluationError> {
        Ok(match value {
            Value::Type => Term::Type,
            Value::Kind => Term::Kind,
            Value::FunctionType {
                parameter_type,
                result,
            } => Term::FunctionType {
                parameter_type: Box::new(self.read_back(parameter_type, depth)?),
                result: self.read_back_closure(result, depth)?,
            },
            Value::Function(closure) => Term::Function {
                body: self.read_back_closure(closure, depth)?,
            },
            Value::Constructor { global, arguments } => {
                self.read_back_application(Term::Global(global.clone()), arguments, depth)?
            }
            Value::Neutral(neutral) => self.read_back_neutral(neutral, depth)?,
            Value::List(elements) => Term::List(self.read_back_values(elements, depth)?),
            Value::Natural(number) => Term::Natural(*number),
            Value::Integer(number) => Term::Integer(*number),
            Value::Text(text) => Term::Text(text.clone()),
        })
    }

    fn read_back_closure(
        &self,
        closure: &Closure,
        depth: usize,
    ) -> Result<Box<Term>, EvaluationError> {
        let value = self.instantiate(closure, fresh_local(depth))?;
        self.read_back(&value, depth + 1).map(Box::new)
    }

    fn read_back_neutral(&self, neutral: &Neutral, depth: usize) -> Result<Term, EvaluationError> {
        Ok(match neutral {
            Neutral::Local(level) => Term::Local(Index(
                reverse_position(depth, level.0).ok_or(EvaluationError::UnboundLevel(*level))?,
            )),
            Neutral::Global(global) => Term::Global(global.clone()),
            Neutral::Apply {
                function,
                arguments,
            } => self.read_back_application(
                self.read_back_neutral(function, depth)?,
                arguments,
                depth,
            )?,
            Neutral::Match {
                subject,
                cases,
                environment,
            } => Term::Match {
                subject: Box::new(self.read_back(subject, depth)?),
                cases: cases
                    .iter()
                    .map(|case| self.read_back_case(case, environment, depth))
                    .collect::<Result<_, _>>()?,
            },
        })
    }

    fn read_back_case(
        &self,
        case: &Case,
        environment: &[Value],
        depth: usize,
    ) -> Result<Case, EvaluationError> {
        let mut environment = environment.to_vec();
        let mut result_depth = depth;
        bind_pattern_locals(&case.pattern, &mut environment, &mut result_depth);
        Ok(Case {
            pattern: case.pattern.clone(),
            result: self.read_back(&self.evaluate(&case.result, &environment)?, result_depth)?,
        })
    }

    fn read_back_application(
        &self,
        function: Term,
        arguments: &[Value],
        depth: usize,
    ) -> Result<Term, EvaluationError> {
        if arguments.is_empty() {
            return Ok(function);
        }
        Ok(Term::Apply {
            function: Box::new(function),
            arguments: self.read_back_values(arguments, depth)?,
        })
    }

    fn read_back_values(
        &self,
        values: &[Value],
        depth: usize,
    ) -> Result<Vec<Term>, EvaluationError> {
        values
            .iter()
            .map(|value| self.read_back(value, depth))
            .collect()
    }
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
