//! Schema information for the argument under the cursor. Never invokes tools.
use std::ops::Range;

use anyx::{Expression, Item, NodeKind, Sequence, TokenKind, TriviaKind};

use crate::{
    Interpreter,
    completion::{expression_tokens, sequence_tokens},
};

/// Parameter information at a UTF-8 byte offset. Overloads may yield several entries.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArgumentInfo {
    pub range: Range<usize>,
    pub label: String,
    pub signature: String,
    pub documentation: Option<String>,
}

impl Interpreter {
    /// Describe the argument containing the cursor, including its expected type.
    /// Works independently of completion snippets and tolerates incomplete input.
    pub fn argument_info(&self, source: &str, offset: usize) -> Vec<ArgumentInfo> {
        if !source.is_char_boundary(offset) {
            return Vec::new();
        }
        let parsed = anyx::parse(source);
        let mut tokens = Vec::new();
        sequence_tokens(&parsed.syntax.sequence, &mut tokens);
        tokens.push(&parsed.syntax.end);
        if tokens.iter().any(|token| {
            token.leading.iter().chain(&token.trailing).any(|trivia| {
                trivia.kind == TriviaKind::Comment
                    && trivia.span.start <= offset
                    && offset <= trivia.span.end
            })
        }) {
            return Vec::new();
        }
        self.sequence_info(source, &parsed.syntax.sequence, offset)
    }

    fn sequence_info(&self, source: &str, sequence: &Sequence, offset: usize) -> Vec<ArgumentInfo> {
        sequence
            .elements
            .iter()
            .find_map(|element| {
                let info = self.expression_info(source, &element.expression, offset);
                (!info.is_empty()).then_some(info)
            })
            .unwrap_or_default()
    }

    fn expression_info(
        &self,
        source: &str,
        expression: &Expression,
        offset: usize,
    ) -> Vec<ArgumentInfo> {
        // Prefer the innermost scoped call, including calls in lists and records.
        let run = match expression {
            Expression::Run(run) => {
                for item in &run.items {
                    if let Item::Group(group) = item {
                        let info = self.sequence_info(source, &group.sequence, offset);
                        if !info.is_empty() {
                            return info;
                        }
                    }
                }
                run
            }
            Expression::LeftGroup(binary) | Expression::RightGroup(binary) => {
                let info = self.expression_info(source, &binary.right, offset);
                if !info.is_empty() {
                    return info;
                }
                let info = self.expression_info(source, &binary.left, offset);
                if !info.is_empty() {
                    return info;
                }
                fn first_run(expression: &Expression) -> &anyx::Run {
                    match expression {
                        Expression::Run(run) => run,
                        Expression::LeftGroup(binary) | Expression::RightGroup(binary) => {
                            first_run(&binary.left)
                        }
                    }
                }
                first_run(&binary.left)
            }
        };
        let [Item::Atom(scope), Item::Atom(slash), ..] = run.items.as_slice() else {
            return Vec::new();
        };
        if scope.kind != TokenKind::Word || slash.text != "/" {
            return Vec::new();
        }
        let Some(tool) = self.tools.get(&scope.text) else {
            return Vec::new();
        };
        let mut tokens = Vec::new();
        // Use concrete token bounds so unfinished strings/groups remain inspectable.
        expression_tokens(expression, &mut tokens);
        let end = tokens.last().map_or(slash.span.end, |token| token.span.end);
        let start = slash.span.end;
        if offset < start || offset > end {
            return Vec::new();
        }
        let parsed = anyx::parse(&source[start..end]);
        let nodes = parsed.syntax.nodes();
        let [node] = nodes.as_slice() else {
            return Vec::new();
        };
        let NodeKind::Call(call) = &node.kind else {
            return Vec::new();
        };
        let Some((index, argument)) = call.arguments.iter().enumerate().find(|(_, argument)| {
            argument.span.start + start <= offset && offset <= argument.span.end + start
        }) else {
            return Vec::new();
        };
        let schema = tool.schema();
        schema
            .methods
            .iter()
            .chain(&schema.built_ins)
            .filter(|method| method.shape.text == call.name_text())
            .filter_map(|method| {
                let parameter = method.shape.parameters.get(index)?;
                Some(ArgumentInfo {
                    range: start + argument.span.start..start + argument.span.end,
                    label: format!(
                        "{}: {}",
                        parameter.name,
                        schema.type_name(&parameter.r#type)
                    ),
                    signature: schema.typed_shape(&method.shape),
                    documentation: method.doc.clone(),
                })
            })
            .collect()
    }
}
