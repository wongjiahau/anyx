//! Whether two patterns can match the same node, which is what makes the
//! order of a `ONE OF` matter. The answer leans towards yes: when it can't
//! tell, it says they can.

use anyx::Span;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use regex_automata::Anchored;
use regex_automata::dfa::{Automaton, dense};
use regex_automata::util::primitives::StateID;
use regex_automata::util::start;

use crate::model::{
    Choice, Element, Grammar, NumberPattern, NumberType, Order, Pattern, Repetition,
    StringCondition, WordKind, expansions, has_groups,
};

pub struct Overlap<'a> {
    grammar: &'a Grammar,
    /// The pairs being compared, to stop at recursive rules. A node is a
    /// finite tree, so a pair that only overlaps through itself does not.
    active: Vec<(usize, usize)>,
    /// The same for [`Overlap::covers`].
    covering: Vec<(usize, usize)>,
    /// Each call with groups, by its address, as a `ONE OF` of the ways of
    /// writing it. They are kept, so that their addresses stay the same.
    expanded: HashMap<usize, Rc<Pattern>>,
}

/// One step of a sequence: a name word of a call, or a node.
#[derive(Clone, Copy)]
enum Atom<'a> {
    NameWord(&'a str),
    /// The name words of a `WORDS`, one at a time.
    NameWords(&'a [StringCondition]),
    Node(&'a Pattern),
    /// Any node: an element of `EACH`, which keeps a node that does not fit
    /// as its error.
    AnyNode,
}

#[derive(Clone, Copy)]
pub struct Step<'a> {
    atom: Atom<'a>,
    optional: bool,
    repeats: bool,
    /// The element or call part it comes from.
    element: &'a Element,
}

/// A point of a sequence laid out as a graph, which a call with groups
/// needs: a step, or where the ways of writing the call part. The graph
/// ends just past its last point.
#[derive(Clone)]
pub enum Junction<'a> {
    Step(Step<'a>),
    /// A group there or not, or the choices of a `ONE OF` between groups:
    /// the point each branch starts at, in the order [`expansions`] lists
    /// them, so that an absent group is the last branch.
    Branch(Vec<usize>),
    /// The end of a branch, on to the point after its group.
    Jump(usize),
}

/// Where two ways of writing a call part: at the branch point `at`, one
/// way takes the branch `first`, the other the branch `second`.
#[derive(Clone, Copy)]
pub struct Fork {
    pub at: usize,
    pub first: usize,
    pub second: usize,
}

/// Where two walks through sequences stand, and whether each has taken its
/// branch of a [`Fork`] yet.
pub type Walking = (usize, usize, bool, bool);

/// The overlap of node patterns found so far in one walk, by their
/// addresses.
pub type Known = HashMap<(usize, usize), bool>;

impl<'a> Overlap<'a> {
    pub fn new(grammar: &'a Grammar) -> Self {
        Overlap {
            grammar,
            active: Vec::new(),
            covering: Vec::new(),
            expanded: HashMap::new(),
        }
    }

    /// A call with groups as a `ONE OF` of the ways of writing it, which
    /// are calls without any.
    fn expanded(&mut self, pattern: &Pattern) -> Option<Rc<Pattern>> {
        let Pattern::SpecificCall(parts) = pattern else {
            return None;
        };
        if !has_groups(parts) {
            return None;
        }
        let key = std::ptr::from_ref(pattern) as usize;
        let expanded = self.expanded.entry(key).or_insert_with(|| {
            let ways = expansions(parts)
                .into_iter()
                .map(|way| Choice {
                    name: String::new(),
                    pattern: Pattern::SpecificCall(way),
                    span: Span::empty(0),
                    documentation: None,
                })
                .collect();
            Rc::new(Pattern::OneOf(ways))
        });
        Some(Rc::clone(expanded))
    }

    /// Whether `general` matches every node `specific` matches, which makes
    /// `general` after `specific` a fallback rather than a competitor. Of
    /// the choices `specific` has, only those `general` can match count. The
    /// answer leans towards no: when it can't tell, it says it doesn't.
    pub fn covers(&mut self, general: &Pattern, specific: &Pattern) -> bool {
        let key = (
            std::ptr::from_ref(general) as usize,
            std::ptr::from_ref(specific) as usize,
        );
        if self.covering.contains(&key) {
            return false;
        }
        self.covering.push(key);
        let covers = self.compare_cover(general, specific);
        self.covering.pop();
        covers
    }

    fn compare_cover(&mut self, general: &Pattern, specific: &Pattern) -> bool {
        if let Some(specific) = self.expanded(specific) {
            return self.covers(general, &specific);
        }
        if let Some(general) = self.expanded(general) {
            return self.covers(&general, specific);
        }
        let grammar = self.grammar;
        if general == specific || *general == Pattern::AnyNode {
            return true;
        }
        match (general, specific) {
            (_, Pattern::Reference(rule)) => self.covers(general, &grammar.rules[*rule].pattern),
            // A choice `general` can't match at all needs no covering.
            (_, Pattern::OneOf(choices)) => choices.iter().all(|choice| {
                !self.patterns(general, &choice.pattern) || self.covers(general, &choice.pattern)
            }),
            // A signed number is the number alone, `+ n` or `- n`: like a
            // choice, of those only the ones `general` can match count.
            (_, Pattern::Number(number)) if number.signed => {
                let unsigned = Pattern::Number(NumberPattern {
                    signed: false,
                    ..*number
                });
                let [plus, minus] = sign_calls(number);
                [unsigned, plus, minus]
                    .iter()
                    .all(|part| !self.patterns(general, part) || self.covers(general, part))
            }
            (Pattern::Reference(rule), _) => self.covers(&grammar.rules[*rule].pattern, specific),
            (Pattern::OneOf(choices), _) => choices
                .iter()
                .any(|choice| self.covers(&choice.pattern, specific)),
            (
                Pattern::AnyCall {
                    name,
                    arguments: None,
                },
                _,
            ) => call_name_covers(name, specific),
            // `ANYFIX CALL [NAME [...], ARGUMENTS: value]` after a call of
            // words and slots: its name holds and each slot is a `value`.
            (
                Pattern::AnyCall {
                    name,
                    arguments: Some(argument),
                },
                Pattern::SpecificCall(parts),
            ) => {
                let slots: Vec<&Element> = parts
                    .iter()
                    .filter(|part| !matches!(part.pattern, Pattern::Word(_)))
                    .collect();
                let count_fits = match argument.repetition {
                    Repetition::One => {
                        matches!(&slots[..], [slot] if slot.repetition == Repetition::One)
                    }
                    Repetition::Optional => match &slots[..] {
                        [] => true,
                        [slot] => matches!(slot.repetition, Repetition::One | Repetition::Optional),
                        _ => false,
                    },
                    Repetition::ZeroOrMore | Repetition::Each => true,
                    Repetition::OneOrMore => {
                        slots.iter().any(|slot| slot.repetition == Repetition::One)
                            || slots
                                .iter()
                                .any(|slot| slot.repetition == Repetition::OneOrMore)
                    }
                };
                count_fits
                    && call_name_covers(name, specific)
                    && slots
                        .iter()
                        .all(|slot| self.covers(&argument.pattern, &slot.pattern))
            }
            _ => false,
        }
    }

    pub fn patterns(&mut self, a: &Pattern, b: &Pattern) -> bool {
        let key = (
            std::ptr::from_ref(a) as usize,
            std::ptr::from_ref(b) as usize,
        );
        if self.active.contains(&key) {
            return false;
        }
        self.active.push(key);
        let overlap = self.compare(a, b);
        self.active.pop();
        overlap
    }

    fn compare(&mut self, a: &Pattern, b: &Pattern) -> bool {
        // Two calls are walked side by side, groups and all, rather than
        // compared way by way.
        if let (Pattern::SpecificCall(_), Pattern::SpecificCall(_)) = (a, b) {
            return self.calls(a, b);
        }
        if let Some(a) = self.expanded(a) {
            return self.patterns(&a, b);
        }
        if let Some(b) = self.expanded(b) {
            return self.patterns(a, &b);
        }
        let grammar = self.grammar;
        match (a, b) {
            (Pattern::Reference(rule), other) => {
                self.patterns(&grammar.rules[*rule].pattern, other)
            }
            (other, Pattern::Reference(rule)) => {
                self.patterns(other, &grammar.rules[*rule].pattern)
            }
            (Pattern::OneOf(choices), other) => choices
                .iter()
                .any(|choice| self.patterns(&choice.pattern, other)),
            (other, Pattern::OneOf(choices)) => choices
                .iter()
                .any(|choice| self.patterns(other, &choice.pattern)),
            (Pattern::AnyNode, _) | (_, Pattern::AnyNode) => true,
            (
                Pattern::Container {
                    kind,
                    elements,
                    order,
                },
                Pattern::Container {
                    kind: other_kind,
                    elements: other_elements,
                    order: other_order,
                },
            ) => {
                // In any order, every element may be missing or repeated,
                // so two brackets of a kind are taken to overlap.
                kind == other_kind
                    && match (order, other_order) {
                        (Order::Written, Order::Written) => {
                            self.sequences(&node_steps(elements), &node_steps(other_elements))
                        }
                        _ => true,
                    }
            }
            // Every `#` line string is also a string, so only the
            // conditions can tell a `LINE STRING` from a `STRING`.
            (Pattern::String(a), Pattern::String(b)) => {
                conditions_overlap(&a.conditions, &b.conditions)
            }
            (Pattern::Number(a), Pattern::Number(b)) => numbers_overlap(a, b),
            // `+ 4` and `- 4` are calls.
            (Pattern::Number(number), other) | (other, Pattern::Number(number))
                if number.signed
                    && !matches!(other, Pattern::Container { .. } | Pattern::String(_)) =>
            {
                sign_calls(number)
                    .iter()
                    .any(|call| self.patterns(call, other))
            }
            (Pattern::Container { .. } | Pattern::String(_) | Pattern::Number(_), _)
            | (_, Pattern::Container { .. } | Pattern::String(_) | Pattern::Number(_)) => false,
            _ => self.calls(a, b) || self.calls(b, a),
        }
    }

    /// Two patterns that match calls. Each pair is handled in one order
    /// only, so the caller tries both.
    fn calls(&mut self, a: &Pattern, b: &Pattern) -> bool {
        match (a, b) {
            (Pattern::Word(a), Pattern::Word(b)) => a == b,
            (Pattern::Word(text), Pattern::AnyWord { kind, conditions }) => {
                kind.is_none_or(|kind| kind == word_kind(text))
                    && conditions.iter().all(|condition| condition.holds(text))
            }
            (Pattern::Word(text), Pattern::AnyCall { name, arguments }) => {
                name.iter().all(|condition| condition.holds(text))
                    && arguments
                        .as_ref()
                        .is_none_or(|arguments| arguments.repetition.may_be_empty())
            }
            (Pattern::Word(_) | Pattern::AnyWord { .. }, Pattern::SpecificCall(parts))
                if has_words(parts) =>
            {
                parts.iter().all(|part| {
                    matches!(part.pattern, Pattern::Words(_)) || part.repetition.may_be_empty()
                })
            }
            (Pattern::Word(_) | Pattern::AnyWord { .. }, Pattern::SpecificCall(parts)) => {
                match only_word(parts) {
                    Some(word) => {
                        let word = Pattern::Word(word.to_string());
                        self.calls(&word, a) || self.calls(a, &word)
                    }
                    None => false,
                }
            }
            (
                Pattern::AnyWord { kind, conditions },
                Pattern::AnyWord {
                    kind: other_kind,
                    conditions: other_conditions,
                },
            ) => {
                (kind.is_none() || other_kind.is_none() || kind == other_kind)
                    && conditions_overlap(conditions, other_conditions)
            }
            (Pattern::AnyWord { conditions, .. }, Pattern::AnyCall { name, arguments }) => {
                conditions_overlap(conditions, name)
                    && arguments
                        .as_ref()
                        .is_none_or(|arguments| arguments.repetition.may_be_empty())
            }
            (
                Pattern::AnyCall { name, arguments },
                Pattern::AnyCall {
                    name: other_name,
                    arguments: other_arguments,
                },
            ) => {
                conditions_overlap(name, other_name)
                    && match (arguments, other_arguments) {
                        (Some(a), Some(b)) => self.sequences(
                            &node_steps(std::slice::from_ref(a)),
                            &node_steps(std::slice::from_ref(b)),
                        ),
                        _ => true,
                    }
            }
            (Pattern::AnyCall { name, arguments }, Pattern::SpecificCall(parts)) => {
                // The words of a `WORDS` are unknown, but its slots and
                // other words are not.
                let fits_a_name = match has_words(parts) {
                    true => conditions_overlap(
                        name,
                        &[StringCondition::MatchingRegex(name_shape(parts))],
                    ),
                    false => possible_names(parts)
                        .iter()
                        .any(|text| name.iter().all(|condition| condition.holds(text))),
                };
                let slots: Vec<Element> = parts
                    .iter()
                    .filter(|part| !matches!(part.pattern, Pattern::Word(_) | Pattern::Words(_)))
                    .cloned()
                    .collect();
                fits_a_name
                    && arguments.as_ref().is_none_or(|arguments| {
                        self.sequences(
                            &node_steps(&slots),
                            &node_steps(std::slice::from_ref(arguments)),
                        )
                    })
            }
            (Pattern::SpecificCall(a), Pattern::SpecificCall(b)) => {
                self.sequences(&call_graph(a), &call_graph(b))
            }
            _ => false,
        }
    }

    /// Whether some list of items fits both sequences.
    fn sequences(&mut self, a: &[Junction], b: &[Junction]) -> bool {
        self.walk(a, b, None, &mut Known::new()).is_some()
    }

    /// The walks through the graph of a call with groups, `graph`, by which
    /// two ways of writing it that part at `fork` fit the same items, if
    /// there are any. `known` keeps the overlap of node patterns from one
    /// call to the next, which is sound only while nothing else is being
    /// compared, as when [`crate::analyze`] starts a comparison.
    pub fn fork(
        &mut self,
        graph: &[Junction],
        fork: Fork,
        known: &mut Known,
    ) -> Option<Vec<Walking>> {
        self.walk(graph, graph, Some(fork), known)
    }

    /// Walks `a` and `b` side by side, both taking each item in turn, to
    /// the ends of both. With a `fork`, the walk through `a` takes its first
    /// branch and the walk through `b` its second. Gives the points passed,
    /// from the start.
    fn walk(
        &mut self,
        a: &[Junction],
        b: &[Junction],
        fork: Option<Fork>,
        known: &mut Known,
    ) -> Option<Vec<Walking>> {
        let start: Walking = (0, 0, fork.is_none(), fork.is_none());
        let mut came_from: HashMap<Walking, Walking> = HashMap::new();
        let mut pending = vec![start];
        let path_to = |end: Walking, came_from: &HashMap<Walking, Walking>| {
            let mut path = vec![end];
            while let Some(before) = path.last().and_then(|at| came_from.get(at)) {
                path.push(*before);
            }
            path.reverse();
            path
        };
        while let Some(at) = pending.pop() {
            let (i, j, a_forked, b_forked) = at;
            if i == a.len() && j == b.len() && a_forked && b_forked {
                return Some(path_to(at, &came_from));
            }
            let mut next: Vec<Walking> = Vec::new();
            let a_branch = fork.map(|fork| (fork.at, fork.first));
            for (i, a_forked) in alone(a, i, a_branch, a_forked) {
                next.push((i, j, a_forked, b_forked));
            }
            let b_branch = fork.map(|fork| (fork.at, fork.second));
            for (j, b_forked) in alone(b, j, b_branch, b_forked) {
                next.push((i, j, a_forked, b_forked));
            }
            if let (Some(Junction::Step(x)), Some(Junction::Step(y))) = (a.get(i), b.get(j))
                && self.atoms(x.atom, y.atom, known)
            {
                for next_i in [i + 1, i] {
                    for next_j in [j + 1, j] {
                        if (next_i == i + 1 || x.repeats) && (next_j == j + 1 || y.repeats) {
                            next.push((next_i, next_j, a_forked, b_forked));
                        }
                    }
                }
            }
            for next in next {
                if next != start && !came_from.contains_key(&next) {
                    came_from.insert(next, at);
                    pending.push(next);
                }
            }
        }
        None
    }

    /// Whether one item may be both atoms.
    fn atoms(&mut self, x: Atom, y: Atom, known: &mut Known) -> bool {
        match (x, y) {
            (Atom::NameWord(x), Atom::NameWord(y)) => x == y,
            (Atom::NameWord(word), Atom::NameWords(conditions))
            | (Atom::NameWords(conditions), Atom::NameWord(word)) => may_be_among(word, conditions),
            (Atom::NameWords(x), Atom::NameWords(y)) => conditions_overlap(x, y),
            (Atom::Node(x), Atom::Node(y)) => {
                let key = (
                    std::ptr::from_ref(x) as usize,
                    std::ptr::from_ref(y) as usize,
                );
                if let Some(overlap) = known.get(&key) {
                    return *overlap;
                }
                let overlap = self.patterns(x, y);
                known.insert(key, overlap);
                overlap
            }
            (Atom::AnyNode, Atom::Node(_) | Atom::AnyNode) | (Atom::Node(_), Atom::AnyNode) => true,
            _ => false,
        }
    }
}

/// Where one walk through `graph` may go from the point `at` without taking
/// an item: past an optional step, into a branch, or on from the end of
/// one. At the branch point of `branch`, it takes only that branch, and
/// notes that it has.
fn alone(
    graph: &[Junction],
    at: usize,
    branch: Option<(usize, usize)>,
    forked: bool,
) -> Vec<(usize, bool)> {
    match graph.get(at) {
        Some(Junction::Step(step)) if step.optional => vec![(at + 1, forked)],
        Some(Junction::Branch(starts)) => starts
            .iter()
            .enumerate()
            .filter_map(|(index, start)| match branch {
                Some((point, taken)) if point == at => (index == taken).then_some((*start, true)),
                _ => Some((*start, forked)),
            })
            .collect(),
        Some(Junction::Jump(to)) => vec![(*to, forked)],
        _ => Vec::new(),
    }
}

/// The parts of the way of writing a call that a walk through its graph
/// took, from the branches it took ([`branches_taken`]).
pub fn way_taken<'a>(graph: &[Junction<'a>], taken: &HashMap<usize, usize>) -> Vec<&'a Element> {
    let mut parts: Vec<&Element> = Vec::new();
    let mut at = 0;
    while let Some(junction) = graph.get(at) {
        at = match junction {
            Junction::Step(step) => {
                if !parts
                    .last()
                    .is_some_and(|last| std::ptr::eq(*last, step.element))
                {
                    parts.push(step.element);
                }
                at + 1
            }
            Junction::Branch(starts) => match taken.get(&at) {
                Some(start) => *start,
                None => starts.last().copied().unwrap_or(at + 1),
            },
            Junction::Jump(to) => *to,
        };
    }
    parts
}

/// The start of the branch a walk took at each branch point it passed.
pub fn branches_taken(
    graph: &[Junction],
    passed: impl Iterator<Item = usize>,
) -> HashMap<usize, usize> {
    let passed: Vec<usize> = passed.collect();
    passed
        .windows(2)
        .filter(|pair| pair[0] != pair[1] && matches!(graph[pair[0]], Junction::Branch(_)))
        .map(|pair| (pair[0], pair[1]))
        .collect()
}

/// A call's parts as a graph: its words and slots in a row, and each group
/// a branch point, with a branch for each way of writing it.
pub fn call_graph(parts: &[Element]) -> Vec<Junction<'_>> {
    let mut graph = Vec::new();
    add_parts(parts, &mut graph);
    graph
}

fn add_parts<'a>(parts: &'a [Element], graph: &mut Vec<Junction<'a>>) {
    for part in parts {
        let branches: Vec<&'a [Element]> = match &part.pattern {
            Pattern::Group(inner) => vec![inner],
            Pattern::PartChoice(choices) => choices
                .iter()
                .map(|choice| match &choice.pattern {
                    Pattern::Group(inner) => inner.as_slice(),
                    _ => &[],
                })
                .collect(),
            _ => {
                graph.extend(
                    part_steps(std::slice::from_ref(part))
                        .into_iter()
                        .map(Junction::Step),
                );
                continue;
            }
        };
        let optional = part.repetition == Repetition::Optional;
        if let ([only], false) = (&branches[..], optional) {
            add_parts(only, graph);
            continue;
        }
        let point = graph.len();
        graph.push(Junction::Branch(Vec::new()));
        let mut starts = Vec::new();
        let mut ends = Vec::new();
        for inner in branches {
            starts.push(graph.len());
            add_parts(inner, graph);
            ends.push(graph.len());
            graph.push(Junction::Jump(0));
        }
        let after = graph.len();
        if optional {
            starts.push(after);
        }
        for end in ends {
            graph[end] = Junction::Jump(after);
        }
        graph[point] = Junction::Branch(starts);
    }
}

/// The steps of a bracket's elements. `ONE OR MORE: x` is `x`, then
/// `ZERO OR MORE: x`.
fn node_steps(elements: &[Element]) -> Vec<Junction<'_>> {
    steps(elements, |element| Atom::Node(&element.pattern))
        .into_iter()
        .map(Junction::Step)
        .collect()
}

/// The steps of a call's parts, where a `WORD` is a name word and a
/// `WORDS` is one or more of them.
fn part_steps(parts: &[Element]) -> Vec<Step<'_>> {
    let mut all = Vec::new();
    for part in parts {
        let one = std::slice::from_ref(part);
        match &part.pattern {
            Pattern::Words(conditions) => all.extend(
                steps(one, |_| Atom::NameWords(conditions))
                    .into_iter()
                    .flat_map(|step| {
                        [
                            step,
                            Step {
                                optional: true,
                                repeats: true,
                                ..step
                            },
                        ]
                    }),
            ),
            // `WORDS "a b"` is a name word for each of its words.
            Pattern::Word(text) => {
                for word in text.split(' ') {
                    all.extend(steps(one, |_| Atom::NameWord(word)));
                }
            }
            pattern => all.extend(steps(one, |_| Atom::Node(pattern))),
        }
    }
    all
}

fn steps<'a>(elements: &'a [Element], atom: impl Fn(&'a Element) -> Atom<'a>) -> Vec<Step<'a>> {
    let mut steps = Vec::new();
    for element in elements {
        let atom = match element.repetition {
            Repetition::Each => Atom::AnyNode,
            _ => atom(element),
        };
        let step = |optional, repeats| Step {
            atom,
            optional,
            repeats,
            element,
        };
        match element.repetition {
            Repetition::One => steps.push(step(false, false)),
            Repetition::Optional => steps.push(step(true, false)),
            Repetition::ZeroOrMore | Repetition::Each => steps.push(step(true, true)),
            Repetition::OneOrMore => steps.extend([step(false, false), step(true, true)]),
        }
    }
    steps
}

/// Whether a call whose name meets every condition in `conditions`, with
/// any arguments, takes every node `specific` matches.
fn call_name_covers(conditions: &[StringCondition], specific: &Pattern) -> bool {
    let all_hold = |text: &str| conditions.iter().all(|condition| condition.holds(text));
    match specific {
        Pattern::Word(text) => all_hold(text),
        Pattern::AnyWord {
            conditions: specific_conditions,
            ..
        }
        | Pattern::AnyCall {
            name: specific_conditions,
            ..
        } => conditions
            .iter()
            .all(|condition| specific_conditions.contains(condition)),
        // The names tried stop at two of a repeated slot, so only a call
        // without one is checked name by name.
        Pattern::SpecificCall(parts) => {
            conditions.is_empty()
                || (!parts.iter().any(|part| part.repetition.may_repeat())
                    && !has_words(parts)
                    && possible_names(parts).iter().all(|name| all_hold(name)))
        }
        _ => false,
    }
}

/// Whether `word` may be one of the words of a `WORDS`. Its conditions
/// are on all the words together, so only those that hold for each word
/// alone are checked, and the answer leans towards yes.
fn may_be_among(word: &str, conditions: &[StringCondition]) -> bool {
    conditions.iter().all(|condition| match condition {
        StringCondition::NoKeyword | StringCondition::NoSlots => condition.holds(word),
        _ => true,
    })
}

/// Whether a call's parts have a `WORDS`, whose words are unknown.
fn has_words(parts: &[Element]) -> bool {
    parts
        .iter()
        .any(|part| matches!(part.pattern, Pattern::Words(_)))
}

/// The word a specific call is when it can be one word and nothing else.
fn only_word(parts: &[Element]) -> Option<&str> {
    let mut words = parts.iter().filter_map(|part| match &part.pattern {
        Pattern::Word(text) => Some(text.as_str()),
        _ => None,
    });
    let word = words.next()?;
    let alone = words.next().is_none()
        && parts
            .iter()
            .all(|part| matches!(part.pattern, Pattern::Word(_)) || part.repetition.may_be_empty());
    alone.then_some(word)
}

/// A regex for every name text a specific call can have: its words, `_`
/// for each slot, and any words but `_` for a `WORDS`. It takes a little
/// more than those, since each space between them may be left out.
fn name_shape(parts: &[Element]) -> String {
    const WORD: &str = "(?:[^ _][^ ]*|_[^ ]+)";
    let shape: String = parts
        .iter()
        .map(|part| match (&part.pattern, part.repetition) {
            (Pattern::Word(text), _) => format!("(?: ?{})", regex::escape(text)),
            (Pattern::Words(_), _) => format!("(?: ?{WORD}(?: {WORD})*)"),
            (_, Repetition::One) => "(?: ?_)".to_string(),
            (_, Repetition::Optional) => "(?: ?_)?".to_string(),
            (_, Repetition::ZeroOrMore | Repetition::Each) => "(?: ?_)*".to_string(),
            (_, Repetition::OneOrMore) => "(?: ?_)+".to_string(),
        })
        .collect();
    format!("^{shape}$")
}

/// Name texts a specific call can have, such as `print`, `print _` and
/// `print _ _`. A slot that repeats is tried up to twice.
fn possible_names(parts: &[Element]) -> Vec<String> {
    let mut names = vec![Vec::new()];
    for part in parts {
        let counts: &[usize] = match (&part.pattern, part.repetition) {
            (Pattern::Word(_), _) | (_, Repetition::One) => &[1],
            (_, Repetition::Optional) => &[0, 1],
            (_, Repetition::ZeroOrMore | Repetition::Each) => &[0, 1, 2],
            (_, Repetition::OneOrMore) => &[1, 2],
        };
        let text = match &part.pattern {
            Pattern::Word(text) => text.as_str(),
            _ => "_",
        };
        names = names
            .iter()
            .flat_map(|name: &Vec<&str>| {
                counts.iter().map(move |count| {
                    let mut name = name.clone();
                    name.extend(std::iter::repeat_n(text, *count));
                    name
                })
            })
            .take(256)
            .collect();
    }
    names.iter().map(|name| name.join(" ")).collect()
}

fn word_kind(text: &str) -> WordKind {
    if anyx::is_symbol_word(text) {
        WordKind::Symbol
    } else if anyx::is_keyword(text) {
        WordKind::Keyword
    } else {
        WordKind::Name
    }
}

/// The calls a `SIGNED` number also matches: `+ _` and `- _` of a number.
fn sign_calls(number: &NumberPattern) -> [Pattern; 2] {
    let call = |sign: &str| {
        let element = |name: Option<&str>, pattern| Element {
            name: name.map(str::to_string),
            repetition: Repetition::One,
            pattern,
            span: Span::empty(0),
            documentation: None,
        };
        let unsigned = NumberPattern {
            signed: false,
            ..*number
        };
        Pattern::SpecificCall(vec![
            element(None, Pattern::Word(sign.to_string())),
            element(Some("number"), Pattern::Number(unsigned)),
        ])
    };
    [call("+"), call("-")]
}

fn numbers_overlap(a: &NumberPattern, b: &NumberPattern) -> bool {
    let highest_minimum = match (a.minimum, b.minimum) {
        (Some(a), Some(b)) if a.as_float() < b.as_float() => Some(b),
        (Some(a), _) => Some(a),
        (None, b) => b,
    };
    let lowest_maximum = match (a.maximum, b.maximum) {
        (Some(a), Some(b)) if a.as_float() > b.as_float() => Some(b),
        (Some(a), _) => Some(a),
        (None, b) => b,
    };
    let both = NumberPattern {
        integer: a.integer || b.integer,
        signed: false,
        minimum: highest_minimum,
        maximum: lowest_maximum,
    };
    match both.number_type() {
        NumberType::Integer {
            minimum, maximum, ..
        } => minimum <= maximum,
        NumberType::Float { minimum, maximum } => minimum <= maximum,
    }
}

/// Whether some text meets every condition of both lists.
fn conditions_overlap(a: &[StringCondition], b: &[StringCondition]) -> bool {
    let all: Vec<&StringCondition> = a.iter().chain(b).collect();
    let exact = all.iter().find_map(|condition| match condition {
        StringCondition::Is(text) => Some(text),
        _ => None,
    });
    if let Some(text) = exact {
        return all.iter().all(|condition| condition.holds(text));
    }
    let regexes: Vec<String> = all
        .iter()
        .map(|condition| match condition {
            StringCondition::Is(text) => format!("^(?:{})$", regex::escape(text)),
            StringCondition::StartsWith(text) => format!("^(?:{})", regex::escape(text)),
            StringCondition::EndsWith(text) => format!("(?:{})$", regex::escape(text)),
            StringCondition::Contains(text) => regex::escape(text),
            StringCondition::MatchingRegex(regex) => regex.clone(),
            StringCondition::NoSlots => "^(?:(?:[^ _][^ ]*|_[^ ]+)(?: |$))*$".to_string(),
            // Every word has a lowercase letter, or no uppercase one.
            StringCondition::NoKeyword => {
                "^(?:(?:[^ ]*\\p{Ll}[^ ]*|[^ \\p{Lu}]*)(?: |$))*$".to_string()
            }
        })
        .collect();
    some_text_matches_all(&regexes)
}

/// Whether some text matches every regex, found by walking their automata
/// side by side. Says yes when a regex is too large to walk.
fn some_text_matches_all(regexes: &[String]) -> bool {
    const MOST_STATES: usize = 20_000;
    let config = dense::Config::new()
        .dfa_size_limit(Some(1 << 22))
        .determinize_size_limit(Some(1 << 22));
    let mut automata = Vec::new();
    let mut starts = Vec::new();
    for regex in regexes {
        let Ok(automaton) = dense::Builder::new().configure(config.clone()).build(regex) else {
            return true;
        };
        let Ok(start) = automaton.start_state(&start::Config::new().anchored(Anchored::No)) else {
            return true;
        };
        automata.push(automaton);
        starts.push(start);
    }
    // A state per automaton, and whether each has matched yet. One that has
    // matched goes back to its start, to keep the states few.
    type Product = (Vec<StateID>, Vec<bool>);
    let first: Product = (starts.clone(), vec![false; automata.len()]);
    let mut seen: HashSet<Product> = HashSet::from([first.clone()]);
    let mut pending = vec![first];
    while let Some((states, matched)) = pending.pop() {
        let matches_at_end = automata.iter().enumerate().all(|(index, automaton)| {
            matched[index] || automaton.is_match_state(automaton.next_eoi_state(states[index]))
        });
        if matches_at_end {
            return true;
        }
        if seen.len() > MOST_STATES {
            return true;
        }
        'bytes: for byte in 0..=255u8 {
            let mut next_states = states.clone();
            let mut next_matched = matched.clone();
            for (index, automaton) in automata.iter().enumerate() {
                if matched[index] {
                    continue;
                }
                let next = automaton.next_state(states[index], byte);
                if automaton.is_quit_state(next) {
                    return true;
                }
                if automaton.is_match_state(next) {
                    next_matched[index] = true;
                    next_states[index] = starts[index];
                } else if automaton.is_dead_state(next) {
                    continue 'bytes;
                } else {
                    next_states[index] = next;
                }
            }
            let next = (next_states, next_matched);
            if seen.insert(next.clone()) {
                pending.push(next);
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grammar::support::NumberValue;

    fn regexes(a: &str, b: &str) -> bool {
        some_text_matches_all(&[a.to_string(), b.to_string()])
    }

    #[test]
    fn regexes_overlap_when_one_text_matches_both() {
        assert!(regexes("^a+$", "^aa$"));
        assert!(regexes("a", "b"));
        assert!(regexes("^[a-z]+$", "^[a-c]x$"));
        assert!(!regexes("^a+$", "^b+$"));
        assert!(!regexes("^[a-z]+ _$", "^[a-z]+$"));
        assert!(!regexes("^a", "^b"));
        assert!(!regexes("a$", "b$"));
        assert!(!regexes("^\\d+$", "^[a-z]+$"));
        assert!(regexes("^\\d+$", "5"));
    }

    #[test]
    fn string_conditions_overlap_when_one_text_meets_all() {
        let starts = |text: &str| StringCondition::StartsWith(text.to_string());
        let is = |text: &str| StringCondition::Is(text.to_string());
        assert!(conditions_overlap(&[starts("ab")], &[starts("a")]));
        assert!(!conditions_overlap(&[starts("ab")], &[starts("b")]));
        assert!(conditions_overlap(&[is("abc")], &[starts("a")]));
        assert!(!conditions_overlap(&[is("abc")], &[is("abd")]));
        assert!(conditions_overlap(&[], &[is("abd")]));
    }

    #[test]
    fn numbers_overlap_when_their_ranges_share_a_value() {
        let number = |integer, minimum: Option<f64>, maximum: Option<f64>| NumberPattern {
            integer,
            signed: false,
            minimum: minimum.map(NumberValue::Float),
            maximum: maximum.map(NumberValue::Float),
        };
        assert!(numbers_overlap(
            &number(false, None, None),
            &number(true, Some(0.0), Some(1.0))
        ));
        assert!(!numbers_overlap(
            &number(false, None, Some(1.0)),
            &number(false, Some(2.0), None)
        ));
        assert!(!numbers_overlap(
            &number(true, None, None),
            &number(false, Some(0.2), Some(0.8))
        ));
    }
}
