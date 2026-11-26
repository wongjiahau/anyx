//! Structural descent for recursive function groups.

use super::evaluation::Definition;
use super::{DeclarationCheckingError, Global, Pattern, Signature, Term};
use crate::lowering::GlobalKind;
use std::collections::{HashMap, HashSet};

pub(super) struct FunctionGroup<'document> {
    pub functions: Vec<(&'document Signature, &'document Term)>,
    pub decreasing_parameter: Option<usize>,
}

impl FunctionGroup<'_> {
    pub fn definition(&self, signature: &Signature, body: &Term) -> Definition {
        let parameter_count = signature.parameter_types.len();
        let mut body = body.clone();
        if self.decreasing_parameter.is_none() {
            for _ in 0..parameter_count {
                body = Term::Function {
                    body: Box::new(body),
                };
            }
        }
        Definition {
            body,
            decreasing_parameter: self.decreasing_parameter,
            parameter_count,
        }
    }
}

pub(super) fn check_termination<'document>(
    functions: &[(&'document Signature, &'document Term)],
) -> Result<Vec<FunctionGroup<'document>>, DeclarationCheckingError> {
    let mut dependencies = Vec::new();
    for (signature, body) in functions {
        let mut references = HashSet::new();
        for term in signature
            .parameter_types
            .iter()
            .chain([&signature.result, *body])
        {
            collect_references(term, &mut references);
        }
        dependencies.push(
            functions
                .iter()
                .enumerate()
                .filter(|(_, (target, _))| references.contains(target.name.as_str()))
                .map(|(position, _)| position)
                .collect::<Vec<_>>(),
        );
    }

    let reachable: Vec<_> = dependencies
        .iter()
        .map(|references| {
            let mut pending = references.clone();
            let mut visited = HashSet::new();
            while let Some(target) = pending.pop() {
                if visited.insert(target) {
                    pending.extend(&dependencies[target]);
                }
            }
            visited
        })
        .collect();

    let mut groups = Vec::new();
    let mut remaining: Vec<_> = (0..functions.len()).collect();
    while !remaining.is_empty() {
        let first = *remaining
            .iter()
            .find(|&&source| {
                remaining.iter().all(|target| {
                    !reachable[source].contains(target) || reachable[*target].contains(&source)
                })
            })
            .expect("a finite dependency graph has a group with no remaining dependencies");
        let mut members = Vec::new();
        remaining.retain(|&position| {
            if position == first || reachable[first].contains(&position) {
                members.push(functions[position]);
                false
            } else {
                true
            }
        });

        let decreasing_parameter = if reachable[first].contains(&first) {
            let names = members
                .iter()
                .map(|(signature, _)| (signature.name.as_str(), signature.parameter_types.len()))
                .collect();
            let parameter_count = members
                .iter()
                .map(|(signature, _)| signature.parameter_types.len())
                .min()
                .expect("a function group has at least one member");
            Some(
                (0..parameter_count)
                    .find(|&position| {
                        members.iter().all(|(signature, body)| {
                            let mut locals =
                                vec![Descent::Unrelated; signature.parameter_types.len()];
                            locals[position] = Descent::Original;
                            decreases(body, &names, position, &mut locals)
                        })
                    })
                    .ok_or_else(|| {
                        DeclarationCheckingError::NonTerminatingFunctions(
                            members
                                .iter()
                                .map(|(signature, _)| signature.name.clone())
                                .collect(),
                        )
                    })?,
            )
        } else {
            None
        };
        groups.push(FunctionGroup {
            functions: members,
            decreasing_parameter,
        });
    }
    Ok(groups)
}

fn collect_references<'term>(term: &'term Term, references: &mut HashSet<&'term str>) {
    if let Term::Global(Global {
        name,
        kind: GlobalKind::Function,
    }) = term
    {
        references.insert(name);
    }
    all_children(term, |child| {
        collect_references(child, references);
        true
    });
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Descent {
    Unrelated,
    Original,
    Smaller,
}

fn descent(term: &Term, locals: &[Descent]) -> Descent {
    if let Term::Local(index) = term {
        return locals
            .iter()
            .rev()
            .nth(index.0)
            .copied()
            .unwrap_or(Descent::Unrelated);
    }
    Descent::Unrelated
}

fn decreases(
    term: &Term,
    recursive: &HashMap<&str, usize>,
    position: usize,
    locals: &mut Vec<Descent>,
) -> bool {
    let recursive_arity = |term: &Term| {
        if let Term::Global(Global {
            name,
            kind: GlobalKind::Function,
        }) = term
        {
            return recursive.get(name.as_str()).copied();
        }
        None
    };
    let original_length = locals.len();
    match term {
        Term::Apply {
            function,
            arguments,
        } => {
            let valid_head = if let Some(arity) = recursive_arity(function) {
                arguments.len() >= arity
                    && arguments
                        .get(position)
                        .is_some_and(|argument| descent(argument, locals) == Descent::Smaller)
            } else {
                decreases(function, recursive, position, locals)
            };
            valid_head
                && arguments
                    .iter()
                    .all(|argument| decreases(argument, recursive, position, locals))
        }
        Term::Global(_) => recursive_arity(term).is_none(),
        Term::FunctionType {
            parameter_type,
            result,
        } => {
            if !decreases(parameter_type, recursive, position, locals) {
                return false;
            }
            locals.push(Descent::Unrelated);
            let valid = decreases(result, recursive, position, locals);
            locals.pop();
            valid
        }
        Term::Function { body } => {
            locals.push(Descent::Unrelated);
            let valid = decreases(body, recursive, position, locals);
            locals.pop();
            valid
        }
        Term::Match { subject, cases } => {
            decreases(subject, recursive, position, locals)
                && cases.iter().all(|case| {
                    bind_pattern(&case.pattern, descent(subject, locals), locals);
                    let valid = decreases(&case.result, recursive, position, locals);
                    locals.truncate(original_length);
                    valid
                })
        }
        _ => all_children(term, |child| decreases(child, recursive, position, locals)),
    }
}

fn bind_pattern(pattern: &Pattern, subject: Descent, locals: &mut Vec<Descent>) {
    match pattern {
        Pattern::Binding => locals.push(subject),
        Pattern::Constructor { arguments, .. } => {
            let field = match subject {
                Descent::Unrelated => Descent::Unrelated,
                Descent::Original | Descent::Smaller => Descent::Smaller,
            };
            for argument in arguments {
                bind_pattern(argument, field, locals);
            }
        }
    }
}

fn all_children<'term>(term: &'term Term, mut visit: impl FnMut(&'term Term) -> bool) -> bool {
    match term {
        Term::FunctionType {
            parameter_type,
            result,
        } => visit(parameter_type) && visit(result),
        Term::Function { body } => visit(body),
        Term::Apply {
            function,
            arguments,
        } => visit(function) && arguments.iter().all(visit),
        Term::Match { subject, cases } => {
            visit(subject) && cases.iter().all(|case| visit(&case.result))
        }
        Term::List(elements) => elements.iter().all(visit),
        Term::Type | Term::Kind | Term::Local(_) | Term::Global(_) | Term::Natural(_) => true,
    }
}
