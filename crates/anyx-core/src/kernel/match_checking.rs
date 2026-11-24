//! Check plain-data patterns and their structural coverage.

use super::super::{Case, Global, Neutral, Pattern, Signature, Term, Value};
use super::{CheckingError, GlobalKind, Scope};
use std::collections::HashMap;

#[derive(Clone, Default)]
pub(super) struct PlainData {
    families: HashMap<String, Vec<Constructor>>,
    owners: HashMap<String, String>,
}

#[derive(Clone)]
struct Constructor {
    name: String,
    arity: usize,
}

impl PlainData {
    pub(super) fn register(&mut self, family: &Signature, constructors: &[Signature]) {
        self.families.insert(
            family.name.clone(),
            constructors
                .iter()
                .map(|constructor| {
                    self.owners
                        .insert(constructor.name.clone(), family.name.clone());
                    Constructor {
                        name: constructor.name.clone(),
                        arity: constructor.parameter_types.len(),
                    }
                })
                .collect(),
        );
    }

    fn constructors(&self, data_type: &Value) -> Result<&[Constructor], CheckingError> {
        if let Value::Neutral(Neutral::Global(Global {
            name,
            kind: GlobalKind::Data,
        })) = data_type
            && let Some(constructors) = self.families.get(name)
        {
            return Ok(constructors);
        }
        Err(CheckingError::MatchNeedsPlainData)
    }

    // Find a row matched by the candidate and by none of the earlier rows.
    fn uncovered(
        &self,
        earlier: &[Vec<&Pattern>],
        candidate: &[&Pattern],
        known_constructors: Option<&[Constructor]>,
    ) -> Option<Vec<Pattern>> {
        let Some((first, remaining)) = candidate.split_first() else {
            return earlier.is_empty().then(Vec::new);
        };
        if let Pattern::Constructor { name, arguments } = first {
            return self.uncovered_constructor(earlier, remaining, name, arguments.iter());
        }

        let heads: Vec<_> = earlier
            .iter()
            .filter_map(|row| match row[0] {
                Pattern::Constructor { name, .. } => Some(name),
                Pattern::Binding => None,
            })
            .collect();
        let constructors = known_constructors.or_else(|| {
            heads
                .first()
                .map(|head| self.families[&self.owners[*head]].as_slice())
        });
        let missing_constructor = if let Some(constructors) = constructors {
            let missing = constructors
                .iter()
                .find(|constructor| !heads.contains(&&constructor.name));
            if missing.is_none() {
                return constructors.iter().find_map(|constructor| {
                    self.uncovered_constructor(
                        earlier,
                        remaining,
                        &constructor.name,
                        std::iter::repeat_n(&Pattern::Binding, constructor.arity),
                    )
                });
            }
            missing
        } else {
            None
        };

        // An unmentioned constructor can only match a binding in this column.
        let defaults: Vec<_> = earlier
            .iter()
            .filter(|row| matches!(row[0], Pattern::Binding))
            .map(|row| row[1..].to_vec())
            .collect();
        let mut uncovered = self.uncovered(&defaults, remaining, None)?;
        let first =
            missing_constructor.map_or(Pattern::Binding, |constructor| Pattern::Constructor {
                name: constructor.name.clone(),
                arguments: vec![Pattern::Binding; constructor.arity],
            });
        uncovered.insert(0, first);
        Some(uncovered)
    }

    fn uncovered_constructor<'pattern>(
        &self,
        earlier: &[Vec<&Pattern>],
        remaining: &[&'pattern Pattern],
        name: &str,
        arguments: impl ExactSizeIterator<Item = &'pattern Pattern>,
    ) -> Option<Vec<Pattern>> {
        let arity = arguments.len();
        let specialized = specialize(earlier, name, arity);
        let candidate: Vec<_> = arguments.chain(remaining.iter().copied()).collect();
        let mut uncovered = self.uncovered(&specialized, &candidate, None)?;
        let arguments = uncovered.drain(..arity).collect();
        uncovered.insert(
            0,
            Pattern::Constructor {
                name: name.into(),
                arguments,
            },
        );
        Some(uncovered)
    }
}

fn specialize<'pattern>(
    rows: &[Vec<&'pattern Pattern>],
    constructor: &str,
    arity: usize,
) -> Vec<Vec<&'pattern Pattern>> {
    rows.iter()
        .filter_map(|row| {
            let fields: Vec<_> = match row[0] {
                Pattern::Binding => vec![&Pattern::Binding; arity],
                Pattern::Constructor { name, arguments } if name == constructor => {
                    arguments.iter().collect()
                }
                Pattern::Constructor { .. } => return None,
            };
            Some(fields.into_iter().chain(row[1..].iter().copied()).collect())
        })
        .collect()
}

impl Scope<'_> {
    pub(super) fn check_match(
        &self,
        subject: &Term,
        cases: &[Case],
        expected: &Value,
    ) -> Result<(), CheckingError> {
        let subject_type = self.synthesize_value(subject)?;
        let constructors = self.plain_data.constructors(&subject_type)?;
        let mut earlier = Vec::new();
        for (position, case) in cases.iter().enumerate() {
            let mut scope = self.clone();
            scope.check_pattern(&case.pattern, &subject_type)?;
            let row = vec![&case.pattern];
            if self
                .plain_data
                .uncovered(&earlier, &row, Some(constructors))
                .is_none()
            {
                return Err(CheckingError::RedundantCase(position + 1));
            }
            scope.check_value(&case.result, expected)?;
            earlier.push(row);
        }

        if let Some(mut uncovered) =
            self.plain_data
                .uncovered(&earlier, &[&Pattern::Binding], Some(constructors))
        {
            let mut binding_names = (1..)
                .map(|number| format!("field{number}"))
                .filter(|name| !self.globals.contains_key(name));
            let pattern = format_pattern(&uncovered.remove(0), &mut binding_names);
            return Err(CheckingError::NonExhaustiveMatch { pattern });
        }
        Ok(())
    }

    fn check_pattern(
        &mut self,
        pattern: &Pattern,
        expected: &Value,
    ) -> Result<Value, CheckingError> {
        let Pattern::Constructor { name, arguments } = pattern else {
            return Ok(self.push_parameter(expected.clone()));
        };
        let constructors = self.plain_data.constructors(expected)?;
        let constructor = constructors
            .iter()
            .find(|constructor| constructor.name == *name)
            .ok_or_else(|| CheckingError::WrongPatternConstructor(name.clone()))?;
        if arguments.len() != constructor.arity {
            return Err(CheckingError::WrongPatternArgumentCount {
                constructor: name.clone(),
                expected: constructor.arity,
                actual: arguments.len(),
            });
        }

        let (global, mut constructor_type) = self.globals[name].clone();
        let mut fields = Vec::new();
        for argument in arguments {
            let Value::FunctionType {
                parameter_type,
                result,
            } = constructor_type
            else {
                return Err(CheckingError::ExpectedFunctionType);
            };
            let field = self.check_pattern(argument, &parameter_type)?;
            constructor_type = result.instantiate(field.clone())?;
            fields.push(field);
        }
        Ok(Value::Constructor {
            global,
            arguments: fields,
        })
    }
}

fn format_pattern(pattern: &Pattern, binding_names: &mut impl Iterator<Item = String>) -> String {
    match pattern {
        Pattern::Binding => binding_names.next().expect("fresh pattern binding"),
        Pattern::Constructor { name, arguments } => {
            let mut arguments = arguments.iter();
            // The canonical call name uses a standalone underscore for each argument slot.
            name.split(' ')
                .map(|part| {
                    if part == "_" {
                        let argument = arguments.next().expect("constructor pattern argument");
                        format!("({})", format_pattern(argument, binding_names))
                    } else {
                        part.to_owned()
                    }
                })
                .collect::<Vec<_>>()
                .join(" ")
        }
    }
}
