//! Schema-derived completion templates. Completion never invokes a tool.

use crate::Interpreter;
use anyx::{Expression, Item, Sequence, Token, TokenKind, TriviaKind};
use anyx_protocol::{
    Schema,
    schema::{Method, Type},
};
use std::ops::Range;

/// A selection within a completion's inserted text, in UTF-8 bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TabStop {
    pub range: Range<usize>,
    pub label: String,
}

/// One menu entry and its editable argument slots.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Completion {
    pub label: String,
    pub detail: String,
    pub documentation: Option<String>,
    pub insert_text: String,
    pub tab_stops: Vec<TabStop>,
}

/// The source range to replace when accepting one of the items.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Completions {
    pub replace: Range<usize>,
    pub items: Vec<Completion>,
}

impl Completions {
    fn empty(offset: usize) -> Self {
        Self {
            replace: offset..offset,
            items: Vec::new(),
        }
    }
}

impl Completion {
    fn plain(text: String) -> Self {
        Self {
            label: text.trim().into(),
            detail: String::new(),
            documentation: None,
            insert_text: text,
            tab_stops: Vec::new(),
        }
    }
}

impl Interpreter {
    /// Suggest scopes and complete method templates at a UTF-8 byte offset.
    /// Includes argument selections and protocol documentation for a UI to show.
    pub fn completions(&self, source: &str, offset: usize) -> Completions {
        if !source.is_char_boundary(offset) {
            return Completions::empty(offset);
        }
        let parsed = anyx::parse(source);
        let mut tokens = Vec::new();
        sequence_tokens(&parsed.syntax.sequence, &mut tokens);
        tokens.push(&parsed.syntax.end);
        if tokens.iter().any(|token| {
            token.leading.iter().chain(&token.trailing).any(|trivia| {
                trivia.kind == TriviaKind::Comment
                    && trivia.span.start < offset
                    && offset <= trivia.span.end
            })
        }) {
            return Completions::empty(offset);
        }
        let mut frames: Vec<Vec<&Token>> = vec![Vec::new()];
        let mut replace = offset..offset;
        let mut prefix = "";
        for token in &tokens {
            if token.kind == TokenKind::Word
                && token.span.start <= offset
                && token.span.end >= offset
            {
                replace = token.span.start..token.span.end;
                prefix = &source[replace.start..offset];
                break;
            }
            if token.span.start >= offset {
                break;
            }
            if token.span.end > offset
                || (matches!(token.kind, TokenKind::String(_))
                    && parsed.errors.iter().any(|error| {
                        error.code == anyx::ErrorCode::UnterminatedString
                            && error.span.start == token.span.start
                    }))
            {
                return Completions::empty(offset);
            }
            match token.kind {
                TokenKind::OpenParen | TokenKind::OpenBracket | TokenKind::OpenBrace => {
                    frames.push(Vec::new())
                }
                TokenKind::CloseParen | TokenKind::CloseBracket | TokenKind::CloseBrace => {
                    if frames.len() == 1 {
                        return Completions::empty(offset);
                    }
                    frames.pop();
                    frames.last_mut().unwrap().push(token);
                }
                TokenKind::Comma | TokenKind::Colon | TokenKind::Dot => {
                    frames.last_mut().unwrap().clear()
                }
                TokenKind::End => {}
                _ => frames.last_mut().unwrap().push(token),
            }
        }
        let current = frames.last().unwrap();
        if current.is_empty() {
            let items = self
                .tools
                .keys()
                .filter(|scope| scope.starts_with(prefix))
                .map(|scope| {
                    Completion::plain(if source[replace.end..].trim_start().starts_with('/') {
                        scope.clone()
                    } else {
                        format!("{scope} / ")
                    })
                })
                .collect();
            return Completions { replace, items };
        }
        let Some(tool) = self.tools.get(&current[0].text) else {
            return Completions::empty(offset);
        };
        if current.len() == 1 {
            let items = if "/".starts_with(prefix) {
                vec![Completion::plain("/ ".into())]
            } else {
                Vec::new()
            };
            return Completions { replace, items };
        }
        if current[1].text != "/" {
            return Completions::empty(offset);
        }
        // When editing before existing arguments/words, replace only the current
        // word. Inserting a whole template there would duplicate the user's suffix.
        let has_suffix = tokens
            .iter()
            .find(|token| token.span.start >= replace.end && token.kind != TokenKind::End)
            .is_some_and(|token| {
                matches!(
                    token.kind,
                    TokenKind::Word
                        | TokenKind::Number
                        | TokenKind::String(_)
                        | TokenKind::OpenParen
                        | TokenKind::OpenBracket
                        | TokenKind::OpenBrace
                )
            });
        let schema = tool.schema();
        let mut items = Vec::new();
        for method in schema
            .methods
            .iter()
            .chain(std::iter::once(anyx_protocol::schema::introspection()))
        {
            let parts: Vec<_> = method.shape.text.split(' ').collect();
            let typed = &current[2..];
            if typed.len() >= parts.len()
                || !typed.iter().zip(&parts).all(|(token, part)| {
                    if *part == "_" {
                        token.kind != TokenKind::Word
                            || matches!(token.text.as_str(), "true" | "false")
                    } else {
                        token.kind == TokenKind::Word && token.text == *part
                    }
                })
            {
                continue;
            }
            let remaining = &parts[typed.len()..];
            if (remaining[0] == "_" && !prefix.is_empty())
                || (remaining[0] != "_" && !remaining[0].starts_with(prefix))
            {
                continue;
            }
            let consumed = parts[..typed.len()]
                .iter()
                .filter(|part| **part == "_")
                .count();
            let mut item = method_completion(schema, method, remaining, consumed);
            if has_suffix && remaining[0] != "_" {
                item.insert_text = remaining[0].into();
                item.tab_stops.clear();
            }
            if offset == replace.end
                && item.tab_stops.is_empty()
                && item.insert_text == source[replace.clone()]
            {
                continue;
            }
            items.push(item);
        }
        items.sort_by(|a, b| a.label.cmp(&b.label).then(a.detail.cmp(&b.detail)));
        Completions { replace, items }
    }
}

fn method_completion(
    schema: &Schema,
    method: &Method,
    parts: &[&str],
    consumed: usize,
) -> Completion {
    let mut item = Completion {
        label: schema.typed_shape(&method.shape),
        detail: method.returns.as_ref().map_or_else(
            || "returns nothing".into(),
            |value| format!("returns {}", schema.type_name(&value.r#type)),
        ),
        documentation: method.doc.clone(),
        insert_text: String::new(),
        tab_stops: Vec::new(),
    };
    let mut parameters = method.shape.parameters.iter().skip(consumed);
    for part in parts {
        if !item.insert_text.is_empty() {
            item.insert_text.push(' ');
        }
        if *part != "_" {
            item.insert_text.push_str(part);
            continue;
        }
        let parameter = parameters
            .next()
            .expect("checked method shape has one parameter per slot");
        let (text, selection) = placeholder(&parameter.r#type, &parameter.name);
        let start = item.insert_text.len();
        item.tab_stops.push(TabStop {
            range: start + selection.start..start + selection.end,
            label: format!(
                "{}: {}",
                parameter.name,
                schema.type_name(&parameter.r#type)
            ),
        });
        item.insert_text.push_str(&text);
    }
    item
}

fn placeholder(r#type: &Type, name: &str) -> (String, Range<usize>) {
    let text = match r#type {
        Type::Text => format!("\"{name}\""),
        Type::Bytes => "\"\"".into(),
        Type::Integer => "0".into(),
        Type::Float => "0.0".into(),
        Type::Boolean => "false".into(),
        Type::Instant => "\"2000-01-01T00:00:00.000Z\"".into(),
        Type::Duration => "0 seconds".into(),
        Type::List(_) => "[]".into(),
        Type::Map(_) => "{}".into(),
        _ => format!("({name})"),
    };
    let range = if text.starts_with(['\"', '[', '{', '(']) {
        1..text.len() - 1
    } else {
        0..text.len()
    };
    (text, range)
}

fn sequence_tokens<'a>(sequence: &'a Sequence, tokens: &mut Vec<&'a Token>) {
    for element in &sequence.elements {
        expression_tokens(&element.expression, tokens);
        if let Some(comma) = &element.comma {
            tokens.push(comma);
        }
    }
}

fn expression_tokens<'a>(expression: &'a Expression, tokens: &mut Vec<&'a Token>) {
    match expression {
        Expression::Run(run) => {
            for item in &run.items {
                match item {
                    Item::Atom(token) | Item::Skipped(token) => tokens.push(token),
                    Item::Group(group) => {
                        tokens.push(&group.open);
                        sequence_tokens(&group.sequence, tokens);
                        if let Some(close) = &group.close {
                            tokens.push(close);
                        }
                    }
                }
            }
        }
        Expression::Dot(binary) | Expression::Colon(binary) => {
            expression_tokens(&binary.left, tokens);
            tokens.push(&binary.operator);
            expression_tokens(&binary.right, tokens);
        }
    }
}
