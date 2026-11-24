//! Checks on a whole grammar: every rule is used, no rule calls itself
//! forever, and no two choices of a `ONE OF` compete for the same input.

use crate::diagnostic::Diagnostic;
use anyx::Span;

use crate::model::{
    ContainerKind, Element, Grammar, Order, Pattern, Repetition, call_picture, has_groups,
};
use crate::overlap::{Fork, Junction, Known, Overlap, branches_taken, call_graph, way_taken};

pub fn analyze(grammar: &Grammar) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    unreachable_rules(grammar, &mut diagnostics);
    endless_recursion(grammar, &mut diagnostics);
    let mut overlap = Overlap::new(grammar);
    if grammar.entry_order == Order::Any {
        competing_elements(&grammar.entry, &mut overlap, &mut diagnostics);
    }
    let roots = grammar
        .entry
        .iter()
        .map(|element| &element.pattern)
        .chain(grammar.rules.iter().map(|rule| &rule.pattern));
    for root in roots {
        competing_choices(root, &mut overlap, &mut diagnostics);
        lone_parentheses(root, &mut diagnostics);
    }
    diagnostics
}

/// The patterns directly inside `pattern`.
fn children(pattern: &Pattern) -> Vec<&Pattern> {
    fn of(elements: &[Element]) -> Vec<&Pattern> {
        elements.iter().map(|element| &element.pattern).collect()
    }
    match pattern {
        Pattern::OneOf(choices) => choices.iter().map(|choice| &choice.pattern).collect(),
        Pattern::Container { elements, .. } => of(elements),
        Pattern::SpecificCall(parts) | Pattern::Group(parts) => of(parts),
        Pattern::PartChoice(choices) => choices.iter().map(|choice| &choice.pattern).collect(),
        Pattern::AnyCall { arguments, .. } => arguments
            .iter()
            .map(|arguments| &arguments.pattern)
            .collect(),
        Pattern::Reference(_)
        | Pattern::Word(_)
        | Pattern::AnyWord { .. }
        | Pattern::String(_)
        | Pattern::Words(_)
        | Pattern::Number(_)
        | Pattern::AnyNode
        | Pattern::Underscore => Vec::new(),
    }
}

fn unreachable_rules(grammar: &Grammar, diagnostics: &mut Vec<Diagnostic>) {
    let mut reached = vec![false; grammar.rules.len()];
    let mut pending: Vec<&Pattern> = grammar
        .entry
        .iter()
        .map(|element| &element.pattern)
        .collect();
    while let Some(pattern) = pending.pop() {
        if let Pattern::Reference(rule) = pattern
            && !std::mem::replace(&mut reached[*rule], true)
        {
            pending.push(&grammar.rules[*rule].pattern);
        }
        pending.extend(children(pattern));
    }
    for (rule, reached) in grammar.rules.iter().zip(reached) {
        if !reached {
            diagnostics.push(Diagnostic::warning(
                rule.name_span,
                format!(
                    "found the rule `{}`, which nothing reachable from `ENTRY` refers to; expected every rule to be used; refer to it or remove it",
                    rule.name
                ),
            ));
        }
    }
}

/// The rules `pattern` hands the very same node to: a reference, or a
/// reference among the choices of a `ONE OF`.
fn same_node_rules(pattern: &Pattern, rules: &mut Vec<usize>) {
    match pattern {
        Pattern::Reference(rule) => rules.push(*rule),
        Pattern::OneOf(choices) => {
            for choice in choices {
                same_node_rules(&choice.pattern, rules);
            }
        }
        _ => {}
    }
}

/// A rule that reaches itself without going into a call or a bracket would
/// decode the same node forever.
fn endless_recursion(grammar: &Grammar, diagnostics: &mut Vec<Diagnostic>) {
    let edges: Vec<Vec<usize>> = grammar
        .rules
        .iter()
        .map(|rule| {
            let mut rules = Vec::new();
            same_node_rules(&rule.pattern, &mut rules);
            rules
        })
        .collect();
    let mut reported = vec![false; grammar.rules.len()];
    for start in 0..grammar.rules.len() {
        if reported[start] {
            continue;
        }
        let Some(cycle) = cycle_through(start, &edges) else {
            continue;
        };
        for rule in &cycle {
            reported[*rule] = true;
        }
        let path: Vec<&str> = cycle
            .iter()
            .chain([&start])
            .map(|rule| grammar.rules[*rule].name.as_str())
            .collect();
        diagnostics.push(Diagnostic::error(
            grammar.rules[start].name_span,
            format!(
                "found the rule `{}` referring to itself with nothing matched in between ({}); expected a call or a bracket on the way, so that each step looks at a smaller node; put the reference inside one, or remove it",
                grammar.rules[start].name,
                path.join(" → ")
            ),
        ));
    }
}

/// A path of rules from `start` back to `start`, if there is one.
fn cycle_through(start: usize, edges: &[Vec<usize>]) -> Option<Vec<usize>> {
    fn visit(
        rule: usize,
        start: usize,
        edges: &[Vec<usize>],
        visited: &mut Vec<bool>,
        path: &mut Vec<usize>,
    ) -> bool {
        path.push(rule);
        for next in &edges[rule] {
            if *next == start {
                return true;
            }
            if !std::mem::replace(&mut visited[*next], true)
                && visit(*next, start, edges, visited, path)
            {
                return true;
            }
        }
        path.pop();
        false
    }
    let mut path = Vec::new();
    let mut visited = vec![false; edges.len()];
    visit(start, start, edges, &mut visited, &mut path).then_some(path)
}

/// One of the alternatives a node is tried against in turn: a choice of a
/// `ONE OF`, or an element of `ANY ORDER`.
struct Alternative<'a> {
    name: &'a str,
    pattern: &'a Pattern,
    span: Span,
}

/// Warns about each alternative that an earlier one takes input from.
fn competing(
    alternatives: &[Alternative],
    kind: &str,
    overlap: &mut Overlap,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for (index, later) in alternatives.iter().enumerate() {
        let earlier = alternatives[..index].iter().find(|earlier| {
            overlap.patterns(earlier.pattern, later.pattern)
                && !overlap.covers(later.pattern, earlier.pattern)
        });
        if let Some(earlier) = earlier {
            diagnostics.push(Diagnostic::warning(
                later.span,
                format!(
                    "found the {kind}s `{}` and `{}` able to match the same input; expected at most one {kind} to fit any input, since `{}` comes first and always wins it; make one of them more specific, or put the more specific one first",
                    earlier.name, later.name, earlier.name
                ),
            ));
        }
    }
}

fn competing_elements(
    elements: &[Element],
    overlap: &mut Overlap,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let alternatives: Vec<Alternative> = elements
        .iter()
        .map(|element| Alternative {
            name: element.name.as_deref().unwrap_or("item"),
            pattern: &element.pattern,
            span: element.span,
        })
        .collect();
    competing(&alternatives, "element", overlap, diagnostics);
}

fn competing_choices(pattern: &Pattern, overlap: &mut Overlap, diagnostics: &mut Vec<Diagnostic>) {
    match pattern {
        Pattern::OneOf(choices) => {
            let alternatives: Vec<Alternative> = choices
                .iter()
                .map(|choice| Alternative {
                    name: &choice.name,
                    pattern: &choice.pattern,
                    span: choice.span,
                })
                .collect();
            competing(&alternatives, "choice", overlap, diagnostics);
        }
        Pattern::Container {
            elements,
            order: Order::Any,
            ..
        } => competing_elements(elements, overlap, diagnostics),
        Pattern::SpecificCall(parts) if has_groups(parts) => {
            competing_ways(parts, overlap, diagnostics)
        }
        _ => {}
    }
    for child in children(pattern) {
        competing_choices(child, overlap, diagnostics);
    }
}

/// Warns when two ways of writing a call with groups can match the same
/// input, as the decoder would take the first of them.
///
/// The checks on groups when a grammar is read (`check_groups` in
/// `lower.rs`) make a word tell each group apart: a group starts with a
/// word, choices start with words of their own, and a group's first word
/// can't come right after it. So where a group is there in one way and not
/// in another, the item after the same parts is that word in one and
/// something else in the other, and the two ways can't fit the same input.
/// That holds only while the parts before the group take the same items in
/// both, and while a word decides. Two cases are left, which this check
/// finds:
/// - a `ONE OF` between groups may have a slot choice, which a node
///   decides, and the parts after it may start with a node too:
///   `F _ _?` and `F _?` of `[WORD "F", OPTIONAL ⁽ONE OF [a ⁽WORD "A",
///   v: STRING], OPTIONAL: STRING]` both fit `F "s"`;
/// - parts before a group may take more or fewer nodes, which moves the
///   group's word to where a later word stands: `_* X _ _*` and
///   `_* _ X _*` of `[_*, OPTIONAL: WORD "X", _, OPTIONAL: WORD "X", _*]`
///   both fit `a X b`.
///
/// Rather than compare every pair of ways, which doubles with each group,
/// it walks the call's graph twice side by side, once for each branch of
/// one branch point, and asks whether both walks can take the same items.
/// That is a walk for each pair of branches at each branch point, each
/// over pairs of the call's points, and each pair of node patterns is
/// compared once.
fn competing_ways(parts: &[Element], overlap: &mut Overlap, diagnostics: &mut Vec<Diagnostic>) {
    let graph = call_graph(parts);
    let mut known = Known::new();
    for (at, junction) in graph.iter().enumerate() {
        let Junction::Branch(starts) = junction else {
            continue;
        };
        for second in 1..starts.len() {
            for first in 0..second {
                let fork = Fork { at, first, second };
                let Some(walked) = overlap.fork(&graph, fork, &mut known) else {
                    continue;
                };
                let one = walked.iter().map(|walking| walking.0);
                let other = walked.iter().map(|walking| walking.1);
                let (one, other) = (branches_taken(&graph, one), branches_taken(&graph, other));
                // The earlier of the two in the order the ways are listed:
                // at the first branch point where they part, the one that
                // took the earlier branch.
                let mut points: Vec<&usize> = one.keys().chain(other.keys()).collect();
                points.sort();
                let swap =
                    points
                        .iter()
                        .find_map(|point| match (one.get(*point), other.get(*point)) {
                            (Some(a), Some(b)) if a != b => Some(a > b),
                            _ => None,
                        });
                let (earlier, later) = match swap {
                    Some(true) => (other, one),
                    _ => (one, other),
                };
                let picture = |taken| {
                    let way: Vec<Element> =
                        way_taken(&graph, &taken).into_iter().cloned().collect();
                    call_picture(&way)
                };
                let (Some(first), Some(last)) = (parts.first(), parts.last()) else {
                    return;
                };
                let span = Span::new(first.span.start, last.span.end);
                diagnostics.push(Diagnostic::warning(
                    span,
                    format!(
                        "found two ways to write this call that can match the same input, `{}` and `{}`; expected each input to fit one way, since the decoder takes the first; give a group a word of its own, or leave out a group",
                        picture(earlier),
                        picture(later)
                    ),
                ));
                return;
            }
        }
    }
}

/// `(x)` only groups: it is the node `x`, with no parentheses left. So a
/// `PARENTHESIS` around exactly one element fits only `(x,)`.
fn lone_parentheses(pattern: &Pattern, diagnostics: &mut Vec<Diagnostic>) {
    if let Pattern::Container {
        kind: ContainerKind::Parenthesis,
        elements,
        order: Order::Written,
    } = pattern
        && let [only] = &elements[..]
        && only.repetition == Repetition::One
    {
        diagnostics.push(Diagnostic::warning(
            only.span,
            "found a `PARENTHESIS` around exactly one element; expected none or several, since `(x)` only groups and is the same node as `x`, which leaves this matching `(x,)` alone; remove the `PARENTHESIS`, as anything may be written in parentheses already",
        ));
    }
    for child in children(pattern) {
        lone_parentheses(child, diagnostics);
    }
}
