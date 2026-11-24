//! First-order index equations established by constructor patterns.

use super::super::{Global, Index, Level, Neutral, Pattern, Term, Value};
use super::{CheckingError, GlobalKind, Scope};

pub(super) fn data_application(value: &Value) -> Option<(&Global, &[Value])> {
    match value {
        Value::Neutral(Neutral::Global(global)) if global.kind == GlobalKind::Data => {
            Some((global, &[]))
        }
        Value::Neutral(Neutral::Apply {
            function,
            arguments,
        }) => match function.as_ref() {
            Neutral::Global(global) if global.kind == GlobalKind::Data => Some((global, arguments)),
            _ => None,
        },
        _ => None,
    }
}

impl Scope<'_> {
    pub(super) fn refine(&self, value: &Value) -> Result<Value, CheckingError> {
        if self.refinements.iter().enumerate().all(|(position, value)| {
            matches!(value, Value::Neutral(Neutral::Local(level)) if level.0 == position)
        }) {
            return Ok(value.clone());
        }
        let depth = self.refinements.len();
        Ok(self.definitions.evaluate(
            &self.definitions.read_back(value, depth)?,
            &self.refinements,
        )?)
    }

    fn solve_local(&mut self, level: Level, value: Value) -> Result<(), CheckingError> {
        let mut replacement: Vec<_> = (0..self.refinements.len())
            .map(super::fresh_local)
            .collect();
        replacement[level.0] = value;
        let definitions = self.definitions;
        let replace = |value: &Value| {
            definitions.evaluate(
                &definitions.read_back(value, replacement.len())?,
                &replacement,
            )
        };
        for value in self.refinements.iter_mut().chain(&mut self.environment) {
            *value = replace(value)?;
        }
        Ok(())
    }

    /// Return false only when the equations prove a contradiction. Opaque
    /// equations can become solvable after another equation refines a local.
    pub(super) fn unify_indices(
        &mut self,
        left: &[Value],
        right: &[Value],
    ) -> Result<bool, CheckingError> {
        let mut pending: Vec<_> = left.iter().cloned().zip(right.iter().cloned()).collect();
        let mut blocked = std::mem::take(&mut self.blocked_indices);
        pending.append(&mut blocked);
        let depth = self.refinements.len();
        while let Some((left, right)) = pending.pop() {
            let left = self.refine(&left)?;
            let right = self.refine(&right)?;
            let left_term = self.definitions.read_back(&left, depth)?;
            let right_term = self.definitions.read_back(&right, depth)?;
            if left_term == right_term {
                continue;
            }

            let local_equation = match (&left, &right) {
                (
                    Value::Neutral(Neutral::Local(left_level)),
                    Value::Neutral(Neutral::Local(right_level)),
                ) if left_level.0 < right_level.0 => Some((*right_level, &left_term, &left)),
                (Value::Neutral(Neutral::Local(level)), _) => Some((*level, &right_term, &right)),
                (_, Value::Neutral(Neutral::Local(level))) => Some((*level, &left_term, &left)),
                _ => None,
            };
            if let Some((level, term, value)) = local_equation {
                let index = Index(depth - 1 - level.0);
                if !mentions_local(term, index) {
                    self.solve_local(level, value.clone())?;
                    pending.append(&mut blocked);
                    continue;
                }
                if occurs_under_rigid_head(level, value) {
                    return Ok(false);
                }
            }

            if let (Some((left_head, left_arguments)), Some((right_head, right_arguments))) =
                (rigid_application(&left), rigid_application(&right))
            {
                if left_head != right_head {
                    return Ok(false);
                }
                if left_arguments.len() == right_arguments.len() {
                    pending.extend(
                        left_arguments
                            .iter()
                            .cloned()
                            .zip(right_arguments.iter().cloned()),
                    );
                    continue;
                }
            }
            blocked.push((left, right));
        }
        self.blocked_indices = blocked;
        Ok(true)
    }
}

fn rigid_application(value: &Value) -> Option<(&Global, &[Value])> {
    match value {
        Value::Constructor { global, arguments } => Some((global, arguments)),
        _ => data_application(value),
    }
}

// A value cannot contain itself through a chain of injective heads.
fn occurs_under_rigid_head(level: Level, value: &Value) -> bool {
    rigid_application(value).is_some_and(|(_, arguments)| {
        arguments.iter().any(|argument| {
            matches!(argument, Value::Neutral(Neutral::Local(local)) if *local == level)
                || occurs_under_rigid_head(level, argument)
        })
    })
}

fn mentions_local(term: &Term, index: Index) -> bool {
    match term {
        Term::Local(local) => *local == index,
        Term::FunctionType {
            parameter_type,
            result,
        } => mentions_local(parameter_type, index) || mentions_local(result, Index(index.0 + 1)),
        Term::Function { body } => mentions_local(body, Index(index.0 + 1)),
        Term::Apply {
            function,
            arguments,
        } => {
            mentions_local(function, index)
                || arguments
                    .iter()
                    .any(|argument| mentions_local(argument, index))
        }
        Term::Match { subject, cases } => {
            mentions_local(subject, index)
                || cases.iter().any(|case| {
                    mentions_local(&case.result, Index(index.0 + binding_count(&case.pattern)))
                })
        }
        Term::List(elements) => elements
            .iter()
            .any(|element| mentions_local(element, index)),
        Term::Type | Term::Global(_) => false,
    }
}

fn binding_count(pattern: &Pattern) -> usize {
    match pattern {
        Pattern::Binding => 1,
        Pattern::Constructor { arguments, .. } => arguments.iter().map(binding_count).sum(),
    }
}
