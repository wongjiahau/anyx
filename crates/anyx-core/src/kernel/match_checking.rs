//! Check constructor patterns and coverage at the subject's indices.

use super::super::{Case, Neutral, Pattern, Signature, Term, Value};
use super::index_unification::data_application;
use super::{CheckingError, Scope};
use std::collections::HashMap;

#[derive(Clone, Default)]
pub(super) struct DataFamilies {
    families: HashMap<String, Family>,
}

#[derive(Clone)]
struct Family {
    arity: usize,
    constructors: Vec<Constructor>,
}

#[derive(Clone)]
struct Constructor {
    name: String,
    arity: usize,
}

impl DataFamilies {
    pub(super) fn register(&mut self, family: &Signature, constructors: &[Signature]) {
        self.families.insert(
            family.name.clone(),
            Family {
                arity: family.parameter_types.len(),
                constructors: constructors
                    .iter()
                    .map(|constructor| Constructor {
                        name: constructor.name.clone(),
                        arity: constructor.parameter_types.len(),
                    })
                    .collect(),
            },
        );
    }

    fn family<'value>(&self, data_type: &'value Value) -> Option<(&Family, &'value [Value])> {
        let (global, indices) = data_application(data_type)?;
        let family = self.families.get(&global.name)?;
        (indices.len() == family.arity).then_some((family, indices))
    }
}

#[derive(Clone)]
struct Column {
    value: Value,
    value_type: Value,
}

impl Scope<'_> {
    pub(super) fn check_match(
        &self,
        subject: &Term,
        cases: &[Case],
        expected: &Value,
    ) -> Result<(), CheckingError> {
        let subject_type = self.synthesize_value(subject)?;
        self.data_families
            .family(&subject_type)
            .ok_or(CheckingError::MatchNeedsData)?;
        let mut coverage = self.clone();
        let column = Column {
            value: coverage.fresh_value(),
            value_type: subject_type.clone(),
        };
        let columns = [column];
        let mut earlier = Vec::new();
        for (position, case) in cases.iter().enumerate() {
            let mut scope = self.clone();
            let subject_value = if matches!(subject, Term::Local(_))
                && let value @ Value::Neutral(Neutral::Local(_)) =
                    self.definitions.evaluate(subject, &self.environment)?
            {
                value
            } else {
                scope.fresh_value()
            };
            scope.check_pattern(
                &case.pattern,
                &Column {
                    value: subject_value,
                    value_type: subject_type.clone(),
                },
            )?;
            let row = vec![&case.pattern];
            if coverage
                .uncovered(&earlier, &row, &columns, true)?
                .is_none()
            {
                return Err(CheckingError::RedundantCase(position + 1));
            }
            scope.check_value(&case.result, expected)?;
            earlier.push(row);
        }

        if let Some(mut uncovered) =
            coverage.uncovered(&earlier, &[&Pattern::Binding], &columns, true)?
        {
            let mut binding_names = (1..)
                .map(|number| format!("field{number}"))
                .filter(|name| !self.globals.contains_key(name));
            let pattern = format_pattern(&uncovered.remove(0), &mut binding_names);
            return Err(CheckingError::NonExhaustiveMatch { pattern });
        }
        Ok(())
    }

    fn check_pattern(&mut self, pattern: &Pattern, column: &Column) -> Result<(), CheckingError> {
        let value_type = self.refine(&column.value_type)?;
        let Pattern::Constructor { name, arguments } = pattern else {
            self.bind_parameter(value_type, self.refine(&column.value)?);
            return Ok(());
        };
        let (family, _) = self
            .data_families
            .family(&value_type)
            .ok_or(CheckingError::MatchNeedsData)?;
        let constructor = family
            .constructors
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

        let fields = self
            .open_constructor(column, constructor)?
            .ok_or_else(|| CheckingError::ImpossiblePattern(name.clone()))?;
        for (argument, field) in arguments.iter().zip(&fields) {
            self.check_pattern(argument, field)?;
        }
        Ok(())
    }

    // Introduce all fields before checking nested patterns so the result
    // indices can determine the types of those fields.
    fn open_constructor(
        &mut self,
        column: &Column,
        constructor: &Constructor,
    ) -> Result<Option<Vec<Column>>, CheckingError> {
        let subject_type = self.refine(&column.value_type)?;
        let (family, indices) = data_application(&subject_type).expect("known data family");
        let (global, constructor_type) = &self.globals[&constructor.name];
        let mut constructor_type = self.definitions.evaluate(constructor_type, &[])?;
        let mut fields = Vec::new();
        for _ in 0..constructor.arity {
            let Value::FunctionType {
                parameter_type,
                result,
            } = constructor_type
            else {
                return Err(CheckingError::ExpectedFunctionType);
            };
            let value = self.fresh_value();
            constructor_type = self.definitions.instantiate(&result, value.clone())?;
            fields.push(Column {
                value,
                value_type: *parameter_type,
            });
        }
        let (result_family, result_indices) =
            data_application(&constructor_type).ok_or(CheckingError::TypeMismatch)?;
        if result_family != family || result_indices.len() != indices.len() {
            return Err(CheckingError::TypeMismatch);
        }
        if !self.unify_indices(indices, result_indices)?
            || !self.unify_indices(
                std::slice::from_ref(&column.value),
                &[Value::Constructor {
                    global: global.clone(),
                    arguments: fields.iter().map(|field| field.value.clone()).collect(),
                }],
            )?
        {
            return Ok(None);
        }
        Ok(Some(fields))
    }

    // Find a row matched by the candidate and by none of the earlier rows.
    fn uncovered(
        &self,
        earlier: &[Vec<&Pattern>],
        candidate: &[&Pattern],
        columns: &[Column],
        match_subject: bool,
    ) -> Result<Option<Vec<Pattern>>, CheckingError> {
        let Some((first, remaining)) = candidate.split_first() else {
            return Ok(earlier.is_empty().then(Vec::new));
        };
        let column = &columns[0];
        let value_type = self.refine(&column.value_type)?;
        let family = self.data_families.family(&value_type);

        if let Pattern::Constructor { name, arguments } = first {
            // The branch checker has already validated the pattern's family
            // and arity. Earlier column refinements may have changed its type.
            let Some((family, _)) = family else {
                return Ok(None);
            };
            let Some(constructor) = family
                .constructors
                .iter()
                .find(|constructor| constructor.name == *name)
            else {
                return Ok(None);
            };
            return self.uncovered_constructor(
                earlier,
                remaining,
                columns,
                constructor,
                arguments.iter(),
            );
        }

        let heads: Vec<_> = earlier
            .iter()
            .filter_map(|row| match row[0] {
                Pattern::Constructor { name, .. } => Some(name),
                Pattern::Binding => None,
            })
            .collect();
        if (match_subject || !heads.is_empty())
            && let Some((family, _)) = family
        {
            // Try unmentioned constructors first to keep missing patterns short.
            for constructor in family
                .constructors
                .iter()
                .filter(|constructor| !heads.contains(&&constructor.name))
                .chain(
                    family
                        .constructors
                        .iter()
                        .filter(|constructor| heads.contains(&&constructor.name)),
                )
            {
                if let Some(witness) = self.uncovered_constructor(
                    earlier,
                    remaining,
                    columns,
                    constructor,
                    std::iter::repeat_n(&Pattern::Binding, constructor.arity),
                )? {
                    return Ok(Some(witness));
                }
            }
            return Ok(None);
        }

        if let Some((family, _)) = family
            && family.arity != 0
        {
            // An uninspected indexed field can be empty by index conflict.
            // Stop at its immediate constructors to bound the search.
            let mut possible = false;
            for constructor in &family.constructors {
                if self
                    .clone()
                    .open_constructor(column, constructor)?
                    .is_some()
                {
                    possible = true;
                    break;
                }
            }
            if !possible {
                return Ok(None);
            }
        }

        // Columns with only bindings need no further constructor inspection.
        let defaults: Vec<_> = earlier
            .iter()
            .filter(|row| matches!(row[0], Pattern::Binding))
            .map(|row| row[1..].to_vec())
            .collect();
        Ok(self
            .uncovered(&defaults, remaining, &columns[1..], false)?
            .map(|mut witness| {
                witness.insert(0, Pattern::Binding);
                witness
            }))
    }

    fn uncovered_constructor<'pattern>(
        &self,
        earlier: &[Vec<&Pattern>],
        remaining: &[&'pattern Pattern],
        columns: &[Column],
        constructor: &Constructor,
        arguments: impl Iterator<Item = &'pattern Pattern>,
    ) -> Result<Option<Vec<Pattern>>, CheckingError> {
        let mut scope = self.clone();
        let Some(mut specialized_columns) = scope.open_constructor(&columns[0], constructor)?
        else {
            return Ok(None);
        };
        specialized_columns.extend_from_slice(&columns[1..]);
        let specialized = specialize(earlier, &constructor.name, constructor.arity);
        let candidate: Vec<_> = arguments.chain(remaining.iter().copied()).collect();
        Ok(scope
            .uncovered(&specialized, &candidate, &specialized_columns, false)?
            .map(|mut witness| {
                let arguments = witness.drain(..constructor.arity).collect();
                witness.insert(
                    0,
                    Pattern::Constructor {
                        name: constructor.name.clone(),
                        arguments,
                    },
                );
                witness
            }))
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
