//! Completion inspects schemas and syntax only; it never invokes a tool.

use std::collections::BTreeSet;

use anyx::{Expression, Item, Sequence, Token, TokenKind, TriviaKind};

use crate::Interpreter;

impl Interpreter {
    /// Complete scope names and literal method words at a UTF-8 byte offset.
    /// Returns the start of the text to replace and its possible replacements.
    /// Arguments are left for the user to fill; no tool is invoked.
    pub fn complete(&self, source: &str, offset: usize) -> (usize, Vec<String>) {
        if !source.is_char_boundary(offset) {
            return (offset, Vec::new());
        }
        let parsed = anyx::parse(source);
        let mut tokens = Vec::new();
        sequence_tokens(&parsed.syntax.sequence, &mut tokens);
        tokens.push(&parsed.syntax.end);
        // A comment at the cursor, including its end before a newline.
        if tokens.iter().any(|token| {
            token.leading.iter().chain(&token.trailing).any(|trivia| {
                trivia.kind == TriviaKind::Comment
                    && trivia.span.start < offset
                    && offset <= trivia.span.end
            })
        }) {
            return (offset, Vec::new());
        }
        let mut frames: Vec<Vec<&Token>> = vec![Vec::new()];
        let mut start = offset;
        let mut prefix = "";
        for token in tokens {
            if token.span.start >= offset {
                break;
            }
            if matches!(token.kind, TokenKind::Word) && token.span.end >= offset {
                start = token.span.start;
                prefix = &source[start..offset];
                break;
            }
            if token.span.end > offset
                || (matches!(token.kind, TokenKind::String(_))
                    && parsed.errors.iter().any(|error| {
                        error.code == anyx::ErrorCode::UnterminatedString
                            && error.span.start == token.span.start
                    }))
            {
                return (offset, Vec::new());
            }
            match token.kind {
                TokenKind::OpenParen | TokenKind::OpenBracket | TokenKind::OpenBrace => {
                    frames.push(Vec::new());
                }
                TokenKind::CloseParen | TokenKind::CloseBracket | TokenKind::CloseBrace => {
                    if frames.len() == 1 {
                        return (offset, Vec::new());
                    }
                    frames.pop();
                    frames.last_mut().unwrap().push(token);
                }
                TokenKind::Comma | TokenKind::Colon | TokenKind::Dot => {
                    frames.last_mut().unwrap().clear();
                }
                TokenKind::End => {}
                _ => frames.last_mut().unwrap().push(token),
            }
        }
        let current = frames.last().unwrap();
        if current.is_empty() {
            return (
                start,
                self.tools
                    .keys()
                    .filter(|scope| scope.starts_with(prefix))
                    .map(|scope| format!("{scope} / "))
                    .collect(),
            );
        }
        let scope = &current[0].text;
        let Some(tool) = self.tools.get(scope) else {
            return (start, Vec::new());
        };
        if current.len() == 1 {
            return (
                start,
                if "/".starts_with(prefix) {
                    vec!["/ ".into()]
                } else {
                    Vec::new()
                },
            );
        }
        if current[1].text != "/" {
            return (start, Vec::new());
        }
        let schema = tool.schema();
        let mut found = BTreeSet::new();
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
                        // Literal values or a complete bracketed argument. Bare choice
                        // expressions need richer type-aware completion in the future.
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
            if remaining[0] == "_" || !remaining[0].starts_with(prefix) {
                continue;
            }
            let words = remaining
                .iter()
                .take_while(|word| **word != "_")
                .copied()
                .collect::<Vec<_>>()
                .join(" ");
            found.insert(format!("{words} "));
        }
        (start, found.into_iter().collect())
    }
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
