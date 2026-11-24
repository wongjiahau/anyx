//! Constructor results and strict positivity for data declarations.

use super::{Declaration, DeclarationCheckingError, Global, Term};
use crate::lowering::GlobalKind;
use std::collections::{HashMap, HashSet};

pub(super) fn check_data_declarations<'document>(
    declarations: impl Iterator<Item = &'document Declaration> + Clone,
) -> Result<(), DeclarationCheckingError> {
    let mut dependencies = HashMap::<&str, Vec<&str>>::new();
    let mut forbidden_occurrences = Vec::new();

    let function_bodies: HashMap<_, _> = declarations
        .clone()
        .filter_map(|declaration| match declaration {
            Declaration::Function { signature, body } => Some((signature.name.as_str(), body)),
            Declaration::Data { .. } => None,
        })
        .collect();
    let visit_references = |term, positive, visit: &mut dyn FnMut(&'document str, bool)| {
        let mut pending = Vec::new();
        visit_global_references(
            term,
            positive,
            &mut |reference, positive| match reference.kind {
                GlobalKind::Data => visit(&reference.name, positive),
                GlobalKind::Function => pending.push(reference.name.as_str()),
                GlobalKind::Constructor => {}
            },
        );

        // A transparent function can hide a recursive occurrence in a field
        // type. Its applications have no established positivity.
        let mut visited = HashSet::new();
        while let Some(function) = pending.pop() {
            if visited.insert(function)
                && let Some(body) = function_bodies.get(function)
            {
                visit_global_references(body, false, &mut |reference, _| match reference.kind {
                    GlobalKind::Data => visit(&reference.name, false),
                    GlobalKind::Function => pending.push(reference.name.as_str()),
                    GlobalKind::Constructor => {}
                });
            }
        }
    };

    for declaration in declarations {
        let Declaration::Data {
            signature,
            constructors,
        } = declaration
        else {
            continue;
        };
        let data = signature.name.as_str();
        if !matches!(signature.result, Term::Type | Term::Kind) {
            return Err(DeclarationCheckingError::DataMustReturnUniverse(
                data.into(),
            ));
        }
        let referenced_data = dependencies.entry(data).or_default();
        for parameter_type in &signature.parameter_types {
            visit_references(parameter_type, false, &mut |reference, _| {
                referenced_data.push(reference);
                forbidden_occurrences.push((data, data, reference));
            });
        }

        for constructor in constructors {
            let mut head = &constructor.result;
            let mut argument_count = 0;
            while let Term::Apply {
                function,
                arguments,
            } = head
            {
                head = function;
                argument_count += arguments.len();
            }
            if !matches!(head, Term::Global(Global { name, kind: GlobalKind::Data }) if name == data)
                || argument_count != signature.parameter_types.len()
            {
                return Err(DeclarationCheckingError::ConstructorMustReturnData {
                    constructor: constructor.name.clone(),
                    data: data.into(),
                });
            }

            let mut record_reference = |reference, positive: bool| {
                referenced_data.push(reference);
                if !positive {
                    forbidden_occurrences.push((data, constructor.name.as_str(), reference));
                }
            };
            for parameter_type in &constructor.parameter_types {
                visit_references(parameter_type, true, &mut record_reference);
            }
            visit_references(&constructor.result, true, &mut record_reference);
        }
    }

    // A forbidden edge is recursive exactly when its target can reach its source.
    for (owner, declaration, reference) in forbidden_occurrences {
        let mut pending = vec![reference];
        let mut visited = HashSet::new();
        while let Some(data) = pending.pop() {
            if data == owner {
                return Err(DeclarationCheckingError::NonPositiveOccurrence {
                    declaration: declaration.into(),
                    data: reference.into(),
                });
            }
            if visited.insert(data)
                && let Some(references) = dependencies.get(data)
            {
                pending.extend(references);
            }
        }
    }

    Ok(())
}

fn visit_global_references<'term>(
    term: &'term Term,
    positive: bool,
    visit: &mut impl FnMut(&'term Global, bool),
) {
    match term {
        Term::Global(global) => visit(global, positive),
        Term::FunctionType {
            parameter_type,
            result,
        } => {
            visit_global_references(parameter_type, false, visit);
            visit_global_references(result, positive, visit);
        }
        Term::Apply {
            function,
            arguments,
        } => {
            visit_global_references(function, positive, visit);
            for argument in arguments {
                visit_global_references(argument, false, visit);
            }
        }
        Term::Function { body } => visit_global_references(body, false, visit),
        Term::Match { subject, cases } => {
            visit_global_references(subject, false, visit);
            for case in cases {
                visit_global_references(&case.result, false, visit);
            }
        }
        Term::List(elements) => {
            for element in elements {
                visit_global_references(element, false, visit);
            }
        }
        Term::Type | Term::Kind | Term::Local(_) => {}
    }
}
