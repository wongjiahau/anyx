//! From the grammar as written to the checked [`Grammar`]: resolves rule
//! references, names fields and choices, and attaches each repetition to
//! the element it repeats. Everything wrong on the way is reported, and
//! lowering carries on to find the rest.

use anyx::{NamePart, NodeKind, Span};

use crate::diagnostic::Diagnostic;
use crate::grammar as ast;
use crate::grammar::support::number_value;
use crate::model::{
    Choice, ContainerKind, Element, Grammar, LayoutRule, MOST_EXPANSIONS, NumberPattern, Order,
    Pattern, Repetition, Rule, StringCondition, StringPattern, WordKind, call_picture,
    expansion_count, expansions, values,
};

/// Names that can't be turned into a Rust field, type or variant.
const UNUSABLE_NAMES: [&str; 3] = ["crate", "self", "super"];

pub fn lower(document: &ast::Document, source: &str) -> (Grammar, Vec<Diagnostic>) {
    let mut lowerer = Lowerer {
        source,
        rule_names: document.rule.iter().map(rule_name).collect(),
        diagnostics: Vec::new(),
        layouts: Vec::new(),
    };
    lowerer.check_rule_names(document);
    let (entry, entry_order) = lowerer.contents(&document.entry.contents, Names::Required);
    let rules = document
        .rule
        .iter()
        .map(|rule| Rule {
            name: rule_name(rule),
            name_span: name_span(rule),
            pattern: lowerer.pattern(named_pattern(rule), Position::Rule),
            documentation: documentation(rule),
        })
        .collect();
    (
        Grammar {
            entry,
            entry_order,
            rules,
            layouts: lowerer.layouts,
        },
        lowerer.diagnostics,
    )
}

/// `key value _`, the call that defines a rule or names a field, is the
/// name `key value`.
fn rule_name(named: &ast::Named) -> String {
    match named {
        ast::Named::Documented { name, .. } => name.value.clone(),
        ast::Named::Plain { name, .. } => {
            let name = &name.text;
            name.strip_suffix(" _").unwrap_or(name).to_string()
        }
    }
}

/// Where the name of a rule, a field or a choice is written.
fn name_span(named: &ast::Named) -> Span {
    match named {
        ast::Named::Documented { name, .. } => name.span,
        ast::Named::Plain { name, .. } => name.span,
    }
}

/// From the name to the end of the pattern, without the documentation.
fn named_span(named: &ast::Named) -> Span {
    Span::new(name_span(named).start, named.span().end)
}

/// What a rule, a field or a choice matches.
fn named_pattern(named: &ast::Named) -> &ast::Pattern {
    match named {
        ast::Named::Documented { pattern, .. } => pattern,
        ast::Named::Plain { arguments, .. } => arguments,
    }
}

/// The `#` line string just above a rule, a field or a choice, each line
/// without the spaces around it.
fn documentation(named: &ast::Named) -> Option<String> {
    let ast::Named::Documented { documentation, .. } = named else {
        return None;
    };
    let text = documentation
        .value
        .value
        .lines()
        .map(str::trim)
        .collect::<Vec<_>>()
        .join("\n");
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_string())
}

struct Lowerer<'a> {
    source: &'a str,
    rule_names: Vec<String>,
    diagnostics: Vec<Diagnostic>,
    layouts: Vec<LayoutRule>,
}

/// Where a pattern stands, which decides whether a bracket becomes a struct.
#[derive(Clone, Copy, PartialEq)]
enum Position {
    /// The whole body of a rule.
    Rule,
    /// A field, a choice or an element.
    Inside,
}

#[derive(Clone, Copy, PartialEq)]
enum Names {
    /// Every element with a value becomes a field, so it needs a name.
    Required,
    /// The sequence has one value, which needs no name.
    Optional,
}

impl Lowerer<'_> {
    fn error(&mut self, span: Span, message: impl Into<String>) {
        self.diagnostics.push(Diagnostic::error(span, message));
    }

    /// Whether something inside `span` is already reported, in which case a
    /// second error about the same text would only repeat it.
    fn has_error_inside(&self, span: Span) -> bool {
        self.diagnostics
            .iter()
            .any(|earlier| span.start <= earlier.span.start && earlier.span.end <= span.end)
    }

    fn text(&self, span: Span) -> &str {
        &self.source[span.start..span.end]
    }

    fn check_rule_names(&mut self, document: &ast::Document) {
        for (index, rule) in document.rule.iter().enumerate() {
            let name = self.rule_names[index].clone();
            let span = name_span(rule);
            if self.rule_names[..index].contains(&name) {
                self.error(
                    span,
                    format!(
                        "found a second rule named `{name}`; expected each rule to be defined once; rename or remove one of them"
                    ),
                );
            } else if name == "document" {
                self.error(
                    span,
                    "found a rule named `document`; expected another name, since `Document` is the type of what `ENTRY` describes; rename the rule",
                );
            } else {
                self.check_usable(&name, span);
            }
        }
    }

    fn check_usable(&mut self, name: &str, span: Span) {
        if UNUSABLE_NAMES.contains(&name) {
            self.error(
                span,
                format!(
                    "found the name `{name}`; expected a name that can be a Rust identifier, which `{name}` can't; choose another name"
                ),
            );
        }
    }

    /// A pattern standing for exactly one node.
    fn pattern(&mut self, pattern: &ast::Pattern, position: Position) -> Pattern {
        match pattern {
            ast::Pattern::Reference(reference) => self.reference(reference),
            ast::Pattern::OneOf(one_of) => Pattern::OneOf(self.choices(&one_of.choices)),
            ast::Pattern::Repetition(ast::Repetition::OptionalGroup { span, .. }) => {
                self.error(
                    *span,
                    "found `OPTIONAL [...]` outside the parts of a call; expected a group of parts only among the parts of an `ANYFIX CALL [...]`, since only a call has name words to leave out together; write `OPTIONAL: pattern` for one element that may be missing",
                );
                Pattern::String(StringPattern::any())
            }
            ast::Pattern::Repetition(repetition) => {
                self.error(
                    repetition.span(),
                    "found a repetition where exactly one node is matched; expected `ZERO OR MORE`, `ONE OR MORE`, `OPTIONAL` and `EACH` only on an element of `ENTRY`, of a bracket or of an `ANYFIX CALL [...]`, or after `ARGUMENTS:`; move it into a list, as in `SQUARE BRACKET [ZERO OR MORE: value]`",
                );
                self.pattern(repetition_of(pattern).1, position)
            }
            ast::Pattern::Container(container) => {
                let (kind, contents) = match container {
                    ast::Container::CurlyBrace { contents, .. } => {
                        (ContainerKind::CurlyBrace, contents)
                    }
                    ast::Container::SquareBracket { contents, .. } => {
                        (ContainerKind::SquareBracket, contents)
                    }
                    ast::Container::Parenthesis { contents, .. } => {
                        (ContainerKind::Parenthesis, contents)
                    }
                };
                let names = match position {
                    Position::Rule => Names::Required,
                    Position::Inside => Names::Optional,
                };
                let (elements, order) = self.contents(contents, names);
                Pattern::Container {
                    kind,
                    elements,
                    order,
                }
            }
            ast::Pattern::SpecificCall(call) => self.specific_call(call),
            ast::Pattern::AnyCall(call) => self.any_call(call),
            ast::Pattern::Word(word) => {
                self.check_word(&word.text.value.value, word.text.span);
                Pattern::Word(word.text.value.value.clone())
            }
            ast::Pattern::AnyWord(any_word) => self.any_word(any_word),
            ast::Pattern::ExactWords(exact) => {
                let words = self.exact_words(exact);
                match words.contains(' ') {
                    false => Pattern::Word(words),
                    // A call of those name words and no arguments.
                    true => Pattern::SpecificCall(vec![Element {
                        name: None,
                        repetition: Repetition::One,
                        pattern: Pattern::Word(words),
                        span: exact.span,
                        documentation: None,
                    }]),
                }
            }
            ast::Pattern::Words(words) => {
                self.error(
                    words.span,
                    "found `WORDS` outside the parts of a call; expected it only among the parts of an `ANYFIX CALL [...]`, since only a call has name words; put it in one, as in `ANYFIX CALL [name: WORDS, value]`",
                );
                self.words(words)
            }
            ast::Pattern::String(string) => Pattern::String(StringPattern {
                line_only: false,
                conditions: match string {
                    ast::String::Any(_) => Vec::new(),
                    ast::String::Exact { text, .. } => {
                        vec![StringCondition::Is(text.value.value.clone())]
                    }
                    ast::String::Conditions { conditions, .. } => {
                        self.string_conditions(conditions)
                    }
                },
            }),
            ast::Pattern::LineString(string) => Pattern::String(StringPattern {
                line_only: true,
                conditions: match string {
                    ast::LineString::Any { .. } => Vec::new(),
                    ast::LineString::Exact { text, .. } => {
                        vec![StringCondition::Is(text.value.value.clone())]
                    }
                    ast::LineString::Conditions { conditions, .. } => {
                        self.string_conditions(conditions)
                    }
                },
            }),
            ast::Pattern::Number(number) => Pattern::Number(self.number(number)),
            ast::Pattern::AnyNode(_) => Pattern::AnyNode,
            ast::Pattern::Underscore(_) => Pattern::Underscore,
        }
    }

    fn reference(&mut self, reference: &ast::Reference) -> Pattern {
        let name = &reference.name.text;
        if let Some(index) = self.rule_names.iter().position(|rule| rule == name) {
            return Pattern::Reference(index);
        }
        let fix = match closest(name, &self.rule_names) {
            Some(close) => format!("write `{close}`, or define `{name}: ...`"),
            None => format!("define `{name}: ...`"),
        };
        self.error(
            reference.span,
            format!(
                "found a reference to `{name}`, which is not a rule; expected the name of a rule of this grammar; {fix}"
            ),
        );
        Pattern::String(StringPattern::any())
    }

    fn choices(&mut self, choices: &[ast::Element]) -> Vec<Choice> {
        let mut lowered: Vec<Choice> = Vec::new();
        for choice in choices {
            let element = self.element(choice);
            if element.repetition != Repetition::One {
                self.error(
                    element.span,
                    "found a repetition as a choice of `ONE OF`; expected each choice to match exactly one node; repeat the whole `ONE OF` instead, as in `ZERO OR MORE: ONE OF [...]`",
                );
            }
            let name = match element.name.or_else(|| self.default_name(&element.pattern)) {
                Some(name) => name,
                None => {
                    let written = one_line(self.text(element.span));
                    if !self.has_error_inside(element.span) {
                        self.error(
                            element.span,
                            format!(
                                "found the choice `{written}` with no name; expected `name: pattern`, since only a rule reference or a `WORD` of letters names itself; add a name, as in `name: {written}`"
                            ),
                        );
                    }
                    continue;
                }
            };
            self.check_usable(&name, element.span);
            if lowered.iter().any(|earlier| earlier.name == name) {
                self.error(
                    element.span,
                    format!(
                        "found a second choice named `{name}`; expected each choice of a `ONE OF` to have its own name; name this one, as in `other {name}: ...`"
                    ),
                );
                continue;
            }
            lowered.push(Choice {
                name,
                pattern: element.pattern,
                span: element.span,
                documentation: element.documentation,
            });
        }
        lowered
    }

    /// The name a pattern has without being given one: a rule reference has
    /// its rule's, and `WORD "null"` is `null`.
    fn default_name(&self, pattern: &Pattern) -> Option<String> {
        match pattern {
            Pattern::Reference(rule) => Some(self.rule_names[*rule].clone()),
            Pattern::Word(text) => {
                let words: Vec<String> = text
                    .split(['_', ' '])
                    .filter(|word| !word.is_empty())
                    .map(str::to_lowercase)
                    .collect();
                let is_name = !words.is_empty()
                    && words.iter().all(|word| {
                        word.starts_with(|c: char| c.is_ascii_lowercase())
                            && word
                                .chars()
                                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
                    });
                is_name.then(|| words.join(" "))
            }
            // `WORDS "sans serif"` is `sans serif`.
            Pattern::SpecificCall(parts) => match &parts[..] {
                [only] if only.is_bare_word() => self.default_name(&only.pattern),
                _ => None,
            },
            _ => None,
        }
    }

    /// An element as written: its name if it has one, and its repetition.
    fn element(&mut self, element: &ast::Element) -> Element {
        let (name, pattern, span, documentation) = match element {
            ast::Element::Field(field) => (
                Some(rule_name(field)),
                named_pattern(field),
                named_span(field),
                documentation(field),
            ),
            ast::Element::Pattern(pattern) => (None, pattern, pattern.span(), None),
        };
        let (repetition, pattern) = repetition_of(pattern);
        Element {
            name,
            repetition,
            pattern: self.pattern(pattern, Position::Inside),
            span,
            documentation,
        }
    }

    /// What a bracket or `ENTRY` holds, and the order it is matched in.
    fn contents(&mut self, contents: &ast::Contents, names: Names) -> (Vec<Element>, Order) {
        match contents {
            ast::Contents::List(list) => (self.sequence(&list.value, names), Order::Written),
            ast::Contents::Element(element) => (
                self.sequence(std::slice::from_ref(element), names),
                Order::Written,
            ),
            ast::Contents::AnyOrder(any_order) => {
                (self.any_order(&any_order.elements.value), Order::Any)
            }
        }
    }

    /// The elements of `ANY ORDER [...]`: each a named field, since every
    /// one is looked for wherever it stands.
    fn any_order(&mut self, elements: &[ast::Element]) -> Vec<Element> {
        let mut elements: Vec<Element> = elements
            .iter()
            .map(|element| self.element(element))
            .collect();
        for element in &elements {
            if element.is_bare_word() {
                self.error(
                    element.span,
                    "found an unnamed `WORD` in `ANY ORDER`; expected every element to be a field, since a word that is only matched has no place when the order is free; name it, as in `flag: WORD \"x\"`",
                );
            }
            if element.repetition == Repetition::Each {
                self.error(
                    element.span,
                    "found `EACH` in `ANY ORDER`; expected `OPTIONAL`, `ZERO OR MORE`, `ONE OR MORE` or no repetition, since a node that fits no element is already kept in `errors` and the rest still decode; use `ZERO OR MORE`",
                );
            }
        }
        self.name_fields(&mut elements, Names::Required);
        for element in &elements {
            if element.name.as_deref() == Some("errors") {
                self.error(
                    element.span,
                    "found a field named `errors` in `ANY ORDER`; expected another name, since `errors` holds the nodes that fit no element; rename the field",
                );
            }
        }
        elements
    }

    /// The elements of a bracket or of `ENTRY`, with their field names.
    fn sequence(&mut self, elements: &[ast::Element], names: Names) -> Vec<Element> {
        let mut elements: Vec<Element> = elements
            .iter()
            .map(|element| self.element(element))
            .collect();
        self.check_each_is_last(&elements);
        let names = match values(&elements).count() {
            0 | 1 => names,
            _ => Names::Required,
        };
        self.name_fields(&mut elements, names);
        elements
    }

    fn name_fields(&mut self, elements: &mut [Element], names: Names) {
        let mut taken: Vec<String> = Vec::new();
        for element in elements {
            if element.is_bare_word() {
                continue;
            }
            // The only value of an inline bracket stays unnamed: the bracket
            // then decodes to that value, with no struct around it.
            if element.name.is_none() && names == Names::Required {
                element.name = match &element.pattern {
                    Pattern::Reference(rule) => Some(self.rule_names[*rule].clone()),
                    _ => None,
                };
            }
            let Some(name) = &element.name else {
                if names == Names::Required && !self.has_error_inside(element.span) {
                    let written = one_line(self.text(element.span));
                    self.error(
                        element.span,
                        format!(
                            "found `{written}` with no name; expected `name: pattern`, since only a rule reference or a `WORD` can go unnamed here; add a name, as in `name: {written}`"
                        ),
                    );
                }
                continue;
            };
            let name = name.clone();
            if name == "span" {
                self.error(
                    element.span,
                    "found a field named `span`; expected another name, since every struct already has a `span`; rename the field",
                );
            }
            self.check_usable(&name, element.span);
            if taken.contains(&name) {
                self.error(
                    element.span,
                    format!(
                        "found a second field named `{name}`; expected each field to have its own name; name this one, as in `other {name}: ...`"
                    ),
                );
            }
            taken.push(name);
        }
    }

    /// `EACH` takes every element left, so nothing after it would ever match.
    fn check_each_is_last(&mut self, elements: &[Element]) {
        let after_each = elements
            .iter()
            .skip_while(|element| element.repetition != Repetition::Each)
            .nth(1);
        if let Some(after) = after_each {
            self.error(
                after.span,
                "found an element after `EACH`; expected `EACH` last, since it takes every element that is left; move it to the end, or use `ZERO OR MORE`, which stops at the first element that does not fit",
            );
        }
    }

    fn specific_call(&mut self, call: &ast::SpecificCall) -> Pattern {
        let (call_parts, span) = match call {
            ast::SpecificCall::Formatted { parts, span, .. }
            | ast::SpecificCall::Plain { parts, span } => (parts, *span),
        };
        let mut parts: Vec<Element> = call_parts
            .iter()
            .flat_map(|part| self.call_part(part))
            .collect();
        self.check_each_is_last(&parts);
        self.name_fields(&mut parts, Names::Required);
        for part in &parts {
            if matches!(part.pattern, Pattern::PartChoice(_))
                && !matches!(part.repetition, Repetition::One | Repetition::Optional)
            {
                self.error(
                    part.span,
                    "found a repeated `ONE OF` between groups of parts; expected it once, or `OPTIONAL`, since each way of writing the call has its name words once; write one `ANYFIX CALL` per shape and join them with `ONE OF`",
                );
            }
        }
        let count = expansion_count(&parts);
        if count > MOST_EXPANSIONS {
            self.error(
                span,
                format!(
                    "found a call with {count} ways of writing it; expected at most {MOST_EXPANSIONS}, since the generator lists every way to tell them apart; split the call into a `ONE OF` of calls, or make fewer of its parts optional"
                ),
            );
            return Pattern::SpecificCall(parts);
        }
        self.check_groups(&parts, &[]);
        // Each way of writing the call is checked as a call of its own;
        // a part found wrong in several of them is reported once.
        let ways = expansions(&parts);
        let mut reported: Vec<Span> = Vec::new();
        for way in &ways {
            for (span, message) in way_errors(way, span, ways.len() > 1) {
                if !reported.contains(&span) {
                    reported.push(span);
                    self.error(span, message);
                }
            }
        }
        if let ast::SpecificCall::Formatted { format, .. } = call {
            self.layout(&ways, format);
        }
        Pattern::SpecificCall(parts)
    }

    /// Checks a call's `FORMAT` against its words and keeps it, once for
    /// each way of writing the call.
    fn layout(&mut self, ways: &[Vec<Element>], format: &[ast::FormatRule]) {
        let words_of = |parts: &[Element]| -> Vec<String> {
            parts
                .iter()
                .filter_map(|part| match &part.pattern {
                    Pattern::Word(text) => {
                        Some(text.split(' ').map(str::to_string).collect::<Vec<_>>())
                    }
                    _ => None,
                })
                .flatten()
                .collect()
        };
        let mut words: Vec<String> = Vec::new();
        for way in ways {
            for word in words_of(way) {
                if !words.contains(&word) {
                    words.push(word);
                }
            }
        }
        let mut break_before: Vec<String> = Vec::new();
        for rule in format {
            let text = rule.text.value.value.clone();
            let span = rule.text.span;
            let is_it =
                |part: &Element| matches!(&part.pattern, Pattern::Word(word) if *word == text);
            let positions: Vec<usize> = ways
                .iter()
                .filter_map(|way| way.iter().position(is_it))
                .collect();
            if positions.is_empty() {
                self.error(
                    span,
                    format!(
                        "found `BREAK BEFORE WORD {text:?}`, which is not a word of the call; expected one of its `WORD` parts{}",
                        match words.is_empty() {
                            true => String::new(),
                            false => format!(
                                ": {}",
                                words.iter().map(|word| format!("{word:?}")).collect::<Vec<_>>().join(", ")
                            ),
                        }
                    ),
                );
            } else if positions.iter().all(|position| *position == 0) {
                self.error(
                    span,
                    format!(
                        "found `BREAK BEFORE WORD {text:?}`, the call's first part; expected a later word, since a call can't break before it starts"
                    ),
                );
            } else if break_before.contains(&text) {
                self.error(
                    span,
                    format!("found `BREAK BEFORE WORD {text:?}` twice; expected each word once"),
                );
            } else {
                break_before.push(text);
            }
        }
        // A break before a word that is not in a call changes nothing, so
        // one rule naming the words every way has serves them all, unless a
        // word to break before is the first part of some way: a call can't
        // break before it starts, so each way then gets a rule of its own.
        let first_in_some_way = ways.iter().any(|way| {
            matches!(way.first(), Some(Element { pattern: Pattern::Word(word), .. }) if break_before.contains(word))
        });
        if !first_in_some_way && let Some(shortest) = ways.last() {
            let in_every_way: Vec<Element> = shortest
                .iter()
                .filter(|part| {
                    ways.iter()
                        .all(|way| way.iter().any(|other| other.span == part.span))
                })
                .cloned()
                .collect();
            let rule = LayoutRule {
                words: words_of(&in_every_way),
                break_before,
            };
            if !self.layouts.contains(&rule) {
                self.layouts.push(rule);
            }
            return;
        }
        for way in ways {
            let rule = LayoutRule {
                words: words_of(way),
                break_before: break_before
                    .iter()
                    .filter(|text| {
                        way.iter()
                            .position(|part| matches!(&part.pattern, Pattern::Word(word) if word == *text))
                            .is_some_and(|position| position > 0)
                    })
                    .cloned()
                    .collect(),
            };
            if !self.layouts.contains(&rule) {
                self.layouts.push(rule);
            }
        }
    }

    /// One of the parts of an `ANYFIX CALL [...]`, where `WORDS` may stand.
    /// `WORDS "a b"` is one part for each of its words.
    fn call_part(&mut self, part: &ast::Element) -> Vec<Element> {
        let (name, pattern, span, documentation) = match part {
            ast::Element::Field(field) => (
                Some(rule_name(field)),
                named_pattern(field),
                named_span(field),
                documentation(field),
            ),
            ast::Element::Pattern(pattern) => (None, pattern, pattern.span(), None),
        };
        if let ast::Pattern::Repetition(ast::Repetition::OptionalGroup { parts, .. }) = pattern {
            return vec![self.optional_group(&parts.value, name, span, documentation)];
        }
        let (repetition, inner) = repetition_of(pattern);
        let element = match inner {
            ast::Pattern::Words(words) => Element {
                name,
                repetition,
                pattern: self.words(words),
                span,
                documentation,
            },
            // One part matching the words in a row; named, it keeps their span.
            ast::Pattern::ExactWords(exact) => Element {
                name,
                repetition,
                pattern: Pattern::Word(self.exact_words(exact)),
                span,
                documentation,
            },
            ast::Pattern::OneOf(one_of) if one_of.choices.value.iter().any(is_word_choice) => {
                Element {
                    name,
                    repetition,
                    pattern: Pattern::PartChoice(self.part_choices(&one_of.choices.value)),
                    span,
                    documentation,
                }
            }
            _ => self.element(part),
        };
        // `OPTIONAL: WORD "PUBLIC"` is a group of that word alone.
        match (&element.pattern, element.repetition) {
            (Pattern::Word(_), Repetition::Optional) => {
                let word = Element {
                    name: None,
                    repetition: Repetition::One,
                    documentation: None,
                    ..element.clone()
                };
                vec![Element {
                    pattern: Pattern::Group(vec![word]),
                    ..element
                }]
            }
            _ => vec![element],
        }
    }

    /// `OPTIONAL [parts]` among the parts of a call. `OPTIONAL [x]` of one
    /// slot is `OPTIONAL: x`.
    fn optional_group(
        &mut self,
        written: &[ast::Element],
        name: Option<String>,
        span: Span,
        documentation: Option<String>,
    ) -> Element {
        let mut parts: Vec<Element> = written
            .iter()
            .flat_map(|part| self.call_part(part))
            .collect();
        if let [only] = &parts[..]
            && only.repetition == Repetition::One
            && is_slot(only)
        {
            let only = parts.remove(0);
            return Element {
                name: name.or(only.name),
                repetition: Repetition::Optional,
                pattern: only.pattern,
                span,
                documentation,
            };
        }
        self.check_each_is_last(&parts);
        let names = match values(&parts).count() {
            0 | 1 => Names::Optional,
            _ => Names::Required,
        };
        self.name_fields(&mut parts, names);
        Element {
            name,
            repetition: Repetition::Optional,
            pattern: Pattern::Group(parts),
            span,
            documentation,
        }
    }

    /// The choices of a `ONE OF` between groups of parts: each one part, a
    /// `WORD` or a slot, held in a group of its own.
    fn part_choices(&mut self, choices: &[ast::Element]) -> Vec<Choice> {
        let mut lowered: Vec<Choice> = Vec::new();
        for choice in choices {
            let mut parts = self.call_part(choice);
            let Some(element) = parts.pop() else { continue };
            if element.repetition != Repetition::One {
                self.error(
                    element.span,
                    "found a repetition as a choice of `ONE OF`; expected each choice to be one part, a `WORD` or a slot; make the whole `ONE OF` optional instead, as in `OPTIONAL: ONE OF [...]`",
                );
                continue;
            }
            if matches!(element.pattern, Pattern::PartChoice(_) | Pattern::Words(_)) {
                self.error(
                    element.span,
                    "found a choice that is not one part; expected a `WORD`, `WORDS \"...\"` or a slot as each choice of a `ONE OF` between groups of parts; join the choices into one `ONE OF`",
                );
                continue;
            }
            let name = match element
                .name
                .clone()
                .or_else(|| self.default_name(&element.pattern))
            {
                Some(name) => name,
                None => {
                    let written = one_line(self.text(element.span));
                    if !self.has_error_inside(element.span) {
                        self.error(
                            element.span,
                            format!(
                                "found the choice `{written}` with no name; expected `name: pattern`, since only a rule reference or a `WORD` of letters names itself; add a name, as in `name: {written}`"
                            ),
                        );
                    }
                    continue;
                }
            };
            self.check_usable(&name, element.span);
            if lowered.iter().any(|earlier| earlier.name == name) {
                self.error(
                    element.span,
                    format!(
                        "found a second choice named `{name}`; expected each choice of a `ONE OF` to have its own name; name this one, as in `other {name}: ...`"
                    ),
                );
                continue;
            }
            let part = Element {
                name: None,
                repetition: Repetition::One,
                pattern: element.pattern,
                span: element.span,
                documentation: None,
            };
            lowered.push(Choice {
                name,
                pattern: Pattern::Group(vec![part]),
                span: element.span,
                documentation: element.documentation,
            });
        }
        lowered
    }

    /// Checks the groups among a call's parts: each is told apart by its
    /// first word, and that word can't also come right after it, since
    /// the decoder takes a group whenever its first word is next. `follow`
    /// is what may come after `parts`.
    fn check_groups(&mut self, parts: &[Element], follow: &[First]) {
        for (index, part) in parts.iter().enumerate() {
            let (rest, rest_may_be_empty) = first_words(&parts[index + 1..]);
            let mut after = rest;
            if rest_may_be_empty {
                after.extend(follow.iter().cloned());
            }
            let leads: Vec<(Option<String>, Span)> = match &part.pattern {
                Pattern::Group(inner) => {
                    if inner
                        .iter()
                        .any(|inner| matches!(inner.pattern, Pattern::Words(_)))
                    {
                        self.error(
                            part.span,
                            "found `WORDS` in a group of parts; expected it only directly among the parts of the call, since the words it takes can't tell whether the group is there; move it out of the group",
                        );
                    }
                    self.check_groups(inner, &after);
                    vec![(lead_word(inner), part.span)]
                }
                Pattern::PartChoice(choices) => choices
                    .iter()
                    .map(|choice| match &choice.pattern {
                        Pattern::Group(inner) => (lead_word(inner), choice.span),
                        _ => (None, choice.span),
                    })
                    .collect(),
                _ => continue,
            };
            if matches!(part.pattern, Pattern::Group(_)) && leads[0].0.is_none() {
                self.error(
                    part.span,
                    "found an optional group that starts with a slot; expected a `WORD` first, since that word tells whether the group is there; put the group's word first, or write the slot alone as `OPTIONAL: pattern`",
                );
                continue;
            }
            let mut seen: Vec<&str> = Vec::new();
            let mut slots = 0;
            for (lead, span) in &leads {
                match lead {
                    None => {
                        slots += 1;
                        if slots == 2 {
                            self.error(
                                *span,
                                "found a second choice that is a slot in a `ONE OF` between groups of parts; expected at most one, since the others are told apart by their word; make this choice a `WORD`, or join the slots into one `ONE OF` rule",
                            );
                        }
                    }
                    Some(word) if seen.contains(&word.as_str()) => self.error(
                        *span,
                        format!(
                            "found a second choice starting with the word `{word}`; expected each choice of a `ONE OF` between groups of parts to start with a word of its own, since that word tells which one is there; give this choice another word"
                        ),
                    ),
                    Some(word) => seen.push(word),
                }
            }
            if part.repetition != Repetition::Optional {
                continue;
            }
            for (lead, span) in &leads {
                let Some(word) = lead else { continue };
                if after.iter().any(|first| first.may_be(word)) {
                    self.error(
                        *span,
                        format!(
                            "found an optional group starting with the word `{word}`, which may also come right after it; expected a first word that can't follow the group, since the decoder takes the group whenever its first word is next; give the group a word of its own"
                        ),
                    );
                }
            }
        }
    }

    /// The words of `WORDS "a b"`, separated by one space each: the text of
    /// a `WORD` that matches them in a row.
    fn exact_words(&mut self, exact: &ast::ExactWords) -> String {
        let text = &exact.text.value.value;
        let span = exact.text.span;
        let words: Vec<&str> = text.split_whitespace().collect();
        if words.is_empty() {
            self.error(
                span,
                "found `WORDS` of no words; expected one or more words separated by spaces, as in `WORDS \"max size\"`",
            );
        }
        for word in &words {
            if !is_one_word(word) {
                self.error(
                    span,
                    format!(
                        "found `{word}` in `WORDS {text:?}`, which is not a word; expected words made of only name characters or only symbol characters, separated by spaces"
                    ),
                );
            }
        }
        words.join(" ")
    }

    fn words(&mut self, words: &ast::Words) -> Pattern {
        let conditions = words.conditions.iter().flat_map(|list| list.iter());
        Pattern::Words(
            conditions
                .filter_map(|condition| self.name_condition(condition))
                .collect(),
        )
    }

    fn name_conditions(&mut self, conditions: &[ast::NameCondition]) -> Vec<StringCondition> {
        conditions
            .iter()
            .filter_map(|condition| self.name_condition(condition))
            .collect()
    }

    fn name_condition(&mut self, condition: &ast::NameCondition) -> Option<StringCondition> {
        match condition {
            ast::NameCondition::NoKeyword { .. } => Some(StringCondition::NoKeyword),
            ast::NameCondition::StringCondition(condition) => self.string_condition(condition),
        }
    }

    fn any_call(&mut self, call: &ast::AnyCall) -> Pattern {
        let mut name = None;
        let mut arguments = None;
        let mut no_arguments = None;
        for condition in call.conditions.iter().flat_map(|list| list.iter()) {
            match condition {
                ast::CallCondition::Name { conditions, span } => {
                    if name.is_some() {
                        self.error(
                            *span,
                            "found a second `NAME`; expected one `NAME [...]` holding every condition on the name; merge the two lists",
                        );
                    }
                    name = Some(self.name_conditions(conditions));
                }
                ast::CallCondition::Arguments { pattern, span } => {
                    if arguments.is_some() {
                        self.error(
                            *span,
                            "found a second `ARGUMENTS`; expected one pattern for the arguments; remove one of them",
                        );
                    }
                    let (repetition, pattern) = repetition_of(pattern);
                    arguments = Some(Box::new(Element {
                        name: None,
                        repetition,
                        pattern: self.pattern(pattern, Position::Inside),
                        span: *span,
                        documentation: None,
                    }));
                }
                ast::CallCondition::NoArguments { span } => no_arguments = Some(*span),
            }
        }
        let mut name = name.unwrap_or_default();
        if let Some(span) = no_arguments {
            if arguments.is_some() {
                self.error(
                    span,
                    "found both `ARGUMENTS` and `NO ARGUMENTS`; expected at most one of them; remove one",
                );
            }
            // A call has an argument for each `_` in its name.
            name.push(StringCondition::NoSlots);
        }
        Pattern::AnyCall { name, arguments }
    }

    fn any_word(&mut self, any_word: &ast::AnyWord) -> Pattern {
        let mut kind = None;
        let mut conditions = Vec::new();
        for condition in any_word.conditions.iter().flat_map(|list| list.iter()) {
            let (this, span) = match condition {
                ast::WordCondition::Keyword(span) => (WordKind::Keyword, *span),
                ast::WordCondition::Name(span) => (WordKind::Name, *span),
                ast::WordCondition::Symbol(span) => (WordKind::Symbol, *span),
                ast::WordCondition::StringCondition(condition) => {
                    conditions.extend(self.string_condition(condition));
                    continue;
                }
            };
            if kind.is_some() {
                self.error(
                    span,
                    "found a second kind of word; expected at most one of `KEYWORD`, `NAME` and `SYMBOL`, since a word is only one of them; use `ONE OF` to allow several kinds",
                );
            }
            kind = Some(this);
        }
        Pattern::AnyWord { kind, conditions }
    }

    fn string_conditions(&mut self, conditions: &[ast::StringCondition]) -> Vec<StringCondition> {
        conditions
            .iter()
            .filter_map(|condition| self.string_condition(condition))
            .collect()
    }

    fn string_condition(&mut self, condition: &ast::StringCondition) -> Option<StringCondition> {
        Some(match condition {
            ast::StringCondition::StartsWith { text, .. } => {
                StringCondition::StartsWith(text.value.value.clone())
            }
            ast::StringCondition::EndsWith { text, .. } => {
                StringCondition::EndsWith(text.value.value.clone())
            }
            ast::StringCondition::Contains { text, .. } => {
                StringCondition::Contains(text.value.value.clone())
            }
            ast::StringCondition::MatchingRegex { regex, .. } => {
                if let Err(error) = regex::Regex::new(&regex.value.value) {
                    let reason = error.to_string();
                    let reason = reason.lines().last().unwrap_or("").trim();
                    self.error(
                        regex.span,
                        format!(
                            "found a regex that does not compile ({reason}); expected the syntax of Rust's `regex` crate; fix the regex, writing each backslash twice"
                        ),
                    );
                    return None;
                }
                StringCondition::MatchingRegex(regex.value.value.clone())
            }
        })
    }

    /// `WORD "x"` must hold what anyx reads as one word.
    fn check_word(&mut self, text: &str, span: Span) {
        if !is_one_word(text) {
            self.error(
                span,
                format!(
                    "found `WORD {text:?}`, which is not one word; expected a single word, made of only name characters or only symbol characters; write one `WORD` for each word"
                ),
            );
        }
    }

    fn number(&mut self, number: &ast::Number) -> NumberPattern {
        let mut pattern = NumberPattern {
            literal: None,
            integer: false,
            signed: false,
            minimum: None,
            maximum: None,
        };
        match number {
            ast::Number::Any(_) => {}
            ast::Number::Exact { value, .. } => {
                let value = number_value(self.text(value.span));
                pattern = NumberPattern {
                    literal: None,
                    integer: matches!(value, crate::grammar::support::NumberValue::Integer(_)),
                    signed: false,
                    minimum: Some(value),
                    maximum: Some(value),
                };
            }
            ast::Number::Conditions { conditions, span } => {
                let mut seen = [false; 5];
                for condition in conditions.iter() {
                    let (index, keyword) = match condition {
                        ast::NumberCondition::WrittenInteger { .. } => (4, "WRITTEN"),
                        ast::NumberCondition::WrittenFloat { .. } => (4, "WRITTEN"),
                        ast::NumberCondition::Integer(_) => (0, "INTEGER"),
                        ast::NumberCondition::Signed(_) => (3, "SIGNED"),
                        ast::NumberCondition::AtLeast { .. } => (1, "AT LEAST"),
                        ast::NumberCondition::AtMost { .. } => (2, "AT MOST"),
                    };
                    if std::mem::replace(&mut seen[index], true) {
                        self.error(
                            condition.span(),
                            format!(
                                "found a second `{keyword}`; expected each condition at most once; remove one of them"
                            ),
                        );
                    }
                    match condition {
                        ast::NumberCondition::WrittenInteger { .. } => {
                            pattern.literal = Some(false)
                        }
                        ast::NumberCondition::WrittenFloat { .. } => pattern.literal = Some(true),
                        ast::NumberCondition::Integer(_) => pattern.integer = true,
                        ast::NumberCondition::Signed(_) => pattern.signed = true,
                        ast::NumberCondition::AtLeast { bound, .. } => {
                            pattern.minimum = Some(number_value(self.text(bound.span)));
                        }
                        ast::NumberCondition::AtMost { bound, .. } => {
                            pattern.maximum = Some(number_value(self.text(bound.span)));
                        }
                    }
                }
                if pattern.literal.is_some() && pattern.signed {
                    self.error(*span, "found `WRITTEN` with `SIGNED`; expected a single number token for `WRITTEN`; remove `SIGNED`");
                }
                if !pattern.is_satisfiable() {
                    self.error(
                        *span,
                        "found conditions that no number meets; expected `AT LEAST` to be no more than `AT MOST`, with an integer between them when `INTEGER` is given; fix the bounds",
                    );
                }
            }
        }
        pattern
    }
}

/// Source text on one line, shortened, for quoting in a message.
fn one_line(text: &str) -> String {
    let joined = text.split_whitespace().collect::<Vec<_>>().join(" ");
    match joined.chars().count() {
        0..=40 => joined,
        _ => format!("{}…", joined.chars().take(40).collect::<String>()),
    }
}

/// The rule name most like `name`, if one is close enough to be a typo.
fn closest<'a>(name: &str, rules: &'a [String]) -> Option<&'a str> {
    rules
        .iter()
        .map(|rule| (edit_distance(name, rule), rule.as_str()))
        .filter(|(distance, _)| *distance <= name.chars().count().div_ceil(3))
        .min_by_key(|(distance, _)| *distance)
        .map(|(_, rule)| rule)
}

fn edit_distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, x) in a.chars().enumerate() {
        let mut previous = row[0];
        row[0] = i + 1;
        for (j, y) in b.iter().enumerate() {
            let substitution = previous + usize::from(x != *y);
            previous = row[j + 1];
            row[j + 1] = substitution.min(previous + 1).min(row[j] + 1);
        }
    }
    row[b.len()]
}

/// A pattern's repetition, and the pattern it repeats: `ZERO OR MORE: x` is
/// `x` zero or more times, and `x` alone is `x` once.
fn repetition_of(pattern: &ast::Pattern) -> (Repetition, &ast::Pattern) {
    match pattern {
        ast::Pattern::Repetition(repetition) => match repetition {
            ast::Repetition::ZeroOrMore { pattern, .. } => (Repetition::ZeroOrMore, pattern),
            ast::Repetition::OneOrMore { pattern, .. } => (Repetition::OneOrMore, pattern),
            ast::Repetition::Optional { pattern, .. } => (Repetition::Optional, pattern),
            ast::Repetition::Each { pattern, .. } => (Repetition::Each, pattern),
            // A group is lowered by `Lowerer::call_part`, and reported
            // anywhere else.
            ast::Repetition::OptionalGroup { .. } => (Repetition::One, pattern),
        },
        pattern => (Repetition::One, pattern),
    }
}

/// Whether anyx reads `text` as exactly one name word or symbol word.
fn is_one_word(text: &str) -> bool {
    let nodes = anyx::parse(text).syntax.nodes();
    matches!(
        &nodes[..],
        [node] if matches!(
            &node.kind,
            NodeKind::Call(call) if matches!(
                &call.name[..],
                [NamePart::Word { text: word, .. }] if word == text
            )
        )
    )
}

/// What is wrong with one way of writing a call: a repeated name word, a
/// name word next to `WORDS`, or one slot and no word. `is_way` when the
/// call has other ways too.
fn way_errors(parts: &[Element], call: Span, is_way: bool) -> Vec<(Span, String)> {
    let mut errors = Vec::new();
    for part in parts {
        let what = match part.pattern {
            Pattern::Word(_) => "WORD",
            Pattern::Words(_) => "WORDS",
            _ => continue,
        };
        if part.repetition != Repetition::One {
            errors.push((
                part.span,
                format!(
                    "found a repeated `{what}` among the parts of a call; expected each name word exactly once; write one `ANYFIX CALL` per shape and join them with `ONE OF`"
                ),
            ));
        }
    }
    for pair in parts.windows(2) {
        let [before, after] = pair else { continue };
        let is_words = |part: &Element| matches!(part.pattern, Pattern::Words(_));
        let is_word = |part: &Element| matches!(part.pattern, Pattern::Word(_));
        if is_words(before) && (is_word(after) || is_words(after))
            || is_word(before) && is_words(after)
        {
            errors.push((
                after.span,
                "found a name word right next to `WORDS`; expected a slot between them, since `WORDS` takes every name word in a row; put a slot between them, or write the words as one `WORDS`".to_string(),
            ));
        }
    }
    let has_word = parts
        .iter()
        .any(|part| matches!(part.pattern, Pattern::Word(_) | Pattern::Words(_)));
    let most_items: usize = parts
        .iter()
        .map(|part| match part.repetition.may_repeat() {
            true => 2,
            false => 1,
        })
        .sum();
    if !has_word && most_items < 2 {
        let message = match is_way {
            false => "found a call of one slot and no word; expected a `WORD`, or at least two parts, since one item alone is never a call; write the part's pattern on its own".to_string(),
            true => format!(
                "found a way to write this call, `{}`, of one slot and no word; expected a `WORD`, or at least two parts, in each way, since one item alone is never a call; make a word of the call always there",
                call_picture(parts)
            ),
        };
        errors.push((call, message));
    }
    errors
}

/// Whether a part fills a slot: neither a name word nor a group.
fn is_slot(part: &Element) -> bool {
    !matches!(
        part.pattern,
        Pattern::Word(_) | Pattern::Words(_) | Pattern::Group(_) | Pattern::PartChoice(_)
    )
}

/// Whether a choice of a `ONE OF` is a `WORD` or `WORDS "..."`, which makes
/// the `ONE OF`, among the parts of a call, a choice between name words.
fn is_word_choice(choice: &ast::Element) -> bool {
    let pattern = match choice {
        ast::Element::Field(field) => named_pattern(field),
        ast::Element::Pattern(pattern) => pattern,
    };
    matches!(
        repetition_of(pattern).1,
        ast::Pattern::Word(_) | ast::Pattern::ExactWords(_)
    )
}

/// The first word of a group, which tells whether it is there.
fn lead_word(parts: &[Element]) -> Option<String> {
    match parts.first().map(|part| &part.pattern) {
        Some(Pattern::Word(text)) => text.split(' ').next().map(str::to_string),
        _ => None,
    }
}

/// A name word that may come first.
#[derive(Clone)]
enum First {
    Word(String),
    /// Any of the words of a `WORDS` that meet its conditions.
    Words(Vec<StringCondition>),
}

impl First {
    /// Whether it may be `word`. The conditions of a `WORDS` are on all
    /// its words together, so only those that hold for each word alone
    /// are checked, and the answer leans towards yes.
    fn may_be(&self, word: &str) -> bool {
        match self {
            First::Word(text) => text == word,
            First::Words(conditions) => conditions.iter().all(|condition| match condition {
                StringCondition::NoKeyword | StringCondition::NoSlots => condition.holds(word),
                _ => true,
            }),
        }
    }
}

/// The name words that may come first among `parts`, and whether all of
/// `parts` may be absent, so that what follows them may come first too.
fn first_words(parts: &[Element]) -> (Vec<First>, bool) {
    let mut found = Vec::new();
    for part in parts {
        let may_be_empty = match &part.pattern {
            Pattern::Word(text) => {
                found.push(First::Word(
                    text.split(' ').next().unwrap_or("").to_string(),
                ));
                false
            }
            Pattern::Words(conditions) => {
                found.push(First::Words(conditions.clone()));
                false
            }
            Pattern::Group(inner) => {
                let (inner, empty) = first_words(inner);
                found.extend(inner);
                empty
            }
            Pattern::PartChoice(choices) => {
                let mut empty = false;
                for choice in choices {
                    if let Pattern::Group(inner) = &choice.pattern {
                        let (inner, inner_empty) = first_words(inner);
                        found.extend(inner);
                        empty |= inner_empty;
                    }
                }
                empty
            }
            _ => false,
        };
        if !(may_be_empty || part.repetition.may_be_empty()) {
            return (found, false);
        }
    }
    (found, true)
}
