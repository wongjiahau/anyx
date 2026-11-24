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

    // Is there a constructor tree matched by this row but by none of the earlier rows?
    fn useful(&self, earlier: &[Vec<&Pattern>], row: &[&Pattern]) -> bool {
        let Some((first, remaining)) = row.split_first() else {
            return earlier.is_empty();
        };
        if let Pattern::Constructor { name, arguments } = first {
            let specialized = specialize(earlier, name, arguments.len());
            let row: Vec<_> = arguments.iter().chain(remaining.iter().copied()).collect();
            return self.useful(&specialized, &row);
        }

        let heads: Vec<_> = earlier
            .iter()
            .filter_map(|row| match row[0] {
                Pattern::Constructor { name, .. } => Some(name),
                Pattern::Binding => None,
            })
            .collect();
        if let Some(head) = heads.first() {
            let constructors = &self.families[&self.owners[*head]];
            if constructors
                .iter()
                .all(|constructor| heads.iter().any(|head| **head == constructor.name))
            {
                return constructors.iter().any(|constructor| {
                    let specialized = specialize(earlier, &constructor.name, constructor.arity);
                    let row: Vec<_> = std::iter::repeat_n(&Pattern::Binding, constructor.arity)
                        .chain(remaining.iter().copied())
                        .collect();
                    self.useful(&specialized, &row)
                });
            }
        }

        // An unmentioned constructor can only match a binding in this column.
        let defaults: Vec<_> = earlier
            .iter()
            .filter(|row| matches!(row[0], Pattern::Binding))
            .map(|row| row[1..].to_vec())
            .collect();
        self.useful(&defaults, remaining)
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
            if constructors.is_empty() || !self.plain_data.useful(&earlier, &row) {
                return Err(CheckingError::RedundantCase(position + 1));
            }
            scope.check_value(&case.result, expected)?;
            earlier.push(row);
        }

        if !constructors.is_empty() && self.plain_data.useful(&earlier, &[&Pattern::Binding]) {
            return Err(CheckingError::NonExhaustiveMatch);
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
