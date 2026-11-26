//! Constructor results and strict positivity for data declarations.

use super::evaluation::{Definition, Definitions, fresh_local};
use super::{Declaration, DeclarationCheckingError, Global, Index, Pattern, Signature, Term};
use crate::lowering::GlobalKind;
use std::collections::{HashMap, HashSet};

/// Unfold each constructor result that names its family through an alias, a
/// function that returns a type, so the result and positivity checks below
/// see the family itself. `get profile RETURNS Private endpoint returning
/// (Profile)` becomes `get profile RETURNS Endpoint returning (Profile)
/// access (bearer)`.
///
/// Only nonrecursive functions unfold, and a call can only name a global, so
/// unfolding terminates even though no body has been checked yet. Soundness
/// does not rest on the alias: the unfolded result is checked as the
/// constructor's own type.
pub(super) fn unfold_constructor_results<'document>(
    declarations: impl Iterator<Item = &'document Declaration> + Clone,
) -> Vec<Declaration> {
    let functions: HashMap<_, _> = declarations
        .clone()
        .filter_map(|declaration| match declaration {
            Declaration::Function { signature, body } => {
                Some((signature.name.as_str(), (signature, body)))
            }
            Declaration::Data { .. } | Declaration::Served { .. } => None,
        })
        .collect();
    let calls = |body: &'document Term| {
        let mut called = Vec::new();
        visit_global_references(body, true, &mut |reference, _| {
            if reference.kind == GlobalKind::Function {
                called.push(reference.name.as_str());
            }
        });
        called
    };
    let recursive = |name: &str| {
        let mut pending = calls(functions[name].1);
        let mut visited = HashSet::new();
        while let Some(called) = pending.pop() {
            if called == name {
                return true;
            }
            if visited.insert(called)
                && let Some((_, body)) = functions.get(called)
            {
                pending.extend(calls(body));
            }
        }
        false
    };
    let aliases = Definitions(
        functions
            .iter()
            .filter(|(name, _)| !recursive(name))
            .map(|(name, (signature, body))| {
                let mut body = (*body).clone();
                for _ in &signature.parameter_types {
                    body = Term::Function {
                        body: Box::new(body),
                    };
                }
                (
                    (*name).to_owned(),
                    Definition {
                        body,
                        decreasing_parameter: None,
                        parameter_count: signature.parameter_types.len(),
                    },
                )
            })
            .collect(),
    );

    declarations
        .map(|declaration| match declaration {
            Declaration::Data {
                signature,
                constructors,
            } => Declaration::Data {
                signature: signature.clone(),
                constructors: constructors
                    .iter()
                    .map(|constructor| unfold_result(&aliases, constructor, &signature.name))
                    .collect(),
            },
            function => function.clone(),
        })
        .collect()
}

fn unfold_result(aliases: &Definitions, constructor: &Signature, data: &str) -> Signature {
    if family_head(&constructor.result).0 == Some(data) {
        return constructor.clone();
    }
    let depth = constructor.parameter_types.len();
    let environment: Vec<_> = (0..depth).map(fresh_local).collect();
    let unfolded = aliases
        .evaluate(&constructor.result, &environment)
        .and_then(|value| aliases.read_back(&value, depth));
    match unfolded {
        Ok(result) => Signature {
            result,
            ..constructor.clone()
        },
        // Leave the result as written so the check below reports it.
        Err(_) => constructor.clone(),
    }
}

/// The data name at the head of an application spine, and its argument count.
fn family_head(mut head: &Term) -> (Option<&str>, usize) {
    let mut argument_count = 0;
    while let Term::Apply {
        function,
        arguments,
    } = head
    {
        head = function;
        argument_count += arguments.len();
    }
    match head {
        Term::Global(Global {
            name,
            kind: GlobalKind::Data,
        }) => (Some(name), argument_count),
        _ => (None, argument_count),
    }
}

pub(super) fn check_data_declarations<'document>(
    declarations: impl Iterator<Item = &'document Declaration> + Clone,
) -> Result<(), DeclarationCheckingError> {
    let mut dependencies = HashMap::<&str, Vec<&str>>::new();
    let mut forbidden_occurrences = Vec::new();

    let function_bodies: HashMap<_, _> = declarations
        .clone()
        .filter_map(|declaration| match declaration {
            Declaration::Function { signature, body } => Some((signature.name.as_str(), body)),
            Declaration::Data { .. } | Declaration::Served { .. } => None,
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
            let (head, argument_count) = family_head(&constructor.result);
            if head != Some(data) || argument_count != signature.parameter_types.len() {
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

/// For each constructor, the binders that stand for the family's parameters.
///
/// A constructor restates every argument of its family as a binder, as in
/// `nothing of (item: Type) RETURNS List of (item)`. Like Coq and Lean, the
/// core does not count a parameter towards a family's universe: a value of
/// `List of (Text)` holds `Text` only as the type it already has. A position
/// is a parameter when it starts the family's arguments and every
/// constructor passes one of its own binders there, both in its result and
/// in every recursive occurrence of the family among its fields.
pub(super) fn parameter_binders(
    signature: &Signature,
    constructors: &[Signature],
) -> Vec<HashSet<usize>> {
    let data = signature.name.as_str();
    let mut binders = vec![HashSet::new(); constructors.len()];
    for position in 0..signature.parameter_types.len() {
        let chosen: Option<Vec<usize>> = constructors
            .iter()
            .map(|constructor| uniform_binder(constructor, data, position))
            .collect();
        let Some(chosen) = chosen else { break };
        for (binders, binder) in binders.iter_mut().zip(chosen) {
            binders.insert(binder);
        }
    }
    binders
}

fn uniform_binder(constructor: &Signature, data: &str, position: usize) -> Option<usize> {
    let depth = constructor.parameter_types.len();
    let binder = match family_arguments(&constructor.result, data)?.get(position)? {
        Term::Local(Index(index)) if *index < depth => depth - 1 - index,
        _ => return None,
    };
    let mut uniform = true;
    for (field, field_type) in constructor.parameter_types.iter().enumerate() {
        visit_occurrences(field_type, data, field, &mut |arguments, depth| {
            let expected = depth.checked_sub(1 + binder).map(Index);
            if arguments.get(position) != expected.map(Term::Local).as_ref() {
                uniform = false;
            }
        });
    }
    uniform.then_some(binder)
}

/// The arguments of `term` when it applies `data`, in written order.
fn family_arguments<'term>(term: &'term Term, data: &str) -> Option<Vec<&'term Term>> {
    let mut head = term;
    let mut spines = Vec::new();
    while let Term::Apply {
        function,
        arguments,
    } = head
    {
        spines.push(arguments);
        head = function;
    }
    match head {
        Term::Global(Global {
            name,
            kind: GlobalKind::Data,
        }) if name == data => Some(spines.into_iter().rev().flatten().collect()),
        _ => None,
    }
}

/// Calls `visit` with the arguments of each occurrence of `data`, and the
/// number of binders in scope there.
fn visit_occurrences(
    term: &Term,
    data: &str,
    depth: usize,
    visit: &mut impl FnMut(Vec<Term>, usize),
) {
    if let Some(arguments) = family_arguments(term, data) {
        visit(
            arguments
                .iter()
                .map(|argument| (*argument).clone())
                .collect(),
            depth,
        );
        for argument in arguments {
            visit_occurrences(argument, data, depth, visit);
        }
        return;
    }
    match term {
        Term::FunctionType {
            parameter_type,
            result,
        } => {
            visit_occurrences(parameter_type, data, depth, visit);
            visit_occurrences(result, data, depth + 1, visit);
        }
        Term::Function { body } => visit_occurrences(body, data, depth + 1, visit),
        Term::Apply {
            function,
            arguments,
        } => {
            visit_occurrences(function, data, depth, visit);
            for argument in arguments {
                visit_occurrences(argument, data, depth, visit);
            }
        }
        Term::Match { subject, cases } => {
            visit_occurrences(subject, data, depth, visit);
            for case in cases {
                let bound = pattern_binders(&case.pattern);
                visit_occurrences(&case.result, data, depth + bound, visit);
            }
        }
        Term::List(elements) => {
            for element in elements {
                visit_occurrences(element, data, depth, visit);
            }
        }
        Term::Type | Term::Kind | Term::Local(_) | Term::Global(_) => {}
    }
}

fn pattern_binders(pattern: &Pattern) -> usize {
    match pattern {
        Pattern::Binding => 1,
        Pattern::Constructor { arguments, .. } => arguments.iter().map(pattern_binders).sum(),
    }
}
