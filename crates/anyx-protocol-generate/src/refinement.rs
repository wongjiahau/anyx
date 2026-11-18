//! Native TypeScript checks emitted from the codec's checked predicate AST.
use anyx_protocol::prelude::BooleanOperation;
use anyx_protocol::refinement::{Expr, Predicate, Test, TextOperand};

pub fn checks(predicate: &Predicate) -> String {
    format!(
        "[{}]",
        predicate
            .tests()
            .iter()
            .flat_map(|test| match test {
                Test::All(_, comparisons) => comparisons.as_slice(),
                test => std::slice::from_ref(test),
            })
            .map(|test| {
                format!(
                    "{{source:{}, words:{}, test:(value: unknown): boolean => {{ {} }} }}",
                    quote(&predicate.source),
                    quote(&test.words()),
                    test.javascript("false")
                )
            })
            .collect::<Vec<_>>()
            .join(",")
    )
}

trait ExpressionJavascript {
    fn javascript(&self, float: bool) -> String;
}
impl ExpressionJavascript for Expr {
    fn javascript(&self, float: bool) -> String {
        if !float {
            let operation = match self {
                Self::Add(a, b) => Some(("+", a, b)),
                Self::Sub(a, b) => Some(("-", a, b)),
                Self::Mul(a, b) => Some(("*", a, b)),
                _ => None,
            };
            if let Some((op, a, b)) = operation {
                return format!(
                    "refinementInteger({op:?}, {}, {})",
                    a.javascript(false),
                    b.javascript(false)
                );
            }
        }
        match self {
            Self::Value(()) | Self::Length(()) => "n".into(),
            Self::Integer(n) => format!("({n}{})", if float { "" } else { "n" }),
            Self::Float(n) => format!("({n:?})"),
            Self::Add(a, b) => format!("({}+{})", a.javascript(float), b.javascript(float)),
            Self::Sub(a, b) => format!("({}-{})", a.javascript(float), b.javascript(float)),
            Self::Mul(a, b) => format!("({}*{})", a.javascript(float), b.javascript(float)),
        }
    }
}
trait TestJavascript {
    fn javascript(&self, invalid: &str) -> String;
}
impl TestJavascript for Test {
    fn javascript(&self, invalid: &str) -> String {
        match self {
            Self::Text(subject, op, pattern) => {
                let operand = |operand: &TextOperand<()>| match operand {
                    TextOperand::Value(()) => "value".into(),
                    TextOperand::Literal(text) => quote(text),
                };
                let a = operand(subject);
                let b = operand(pattern);
                let method = match *op {
                    "starts with" => "startsWith",
                    "ends with" => "endsWith",
                    _ => "includes",
                };
                if matches!(subject, TextOperand::Value(()))
                    && matches!(pattern, TextOperand::Literal(_))
                    && invalid == "false"
                {
                    format!("return typeof value === 'string' && value.{method}({b});")
                } else {
                    format!(
                        "const a = {a}; const b = {b}; return typeof a === 'string' && typeof b === 'string' ? a.{method}(b) : {invalid};"
                    )
                }
            }
            Self::Flag(()) => format!(
                "const kind = value !== null && typeof value === 'object' ? (value as {{kind?: unknown}}).kind : undefined; return kind === 'true' ? true : kind === 'false' ? false : {invalid};"
            ),
            Self::Literal(value) => format!("return {value};"),
            Self::All(_, tests) | Self::Boolean(_, tests) => {
                let locals = tests.iter().enumerate().map(|(index, test)| {
                    format!("const v{index} = (() => {{ {} }})(); if (v{index} === null) return {invalid};", test.javascript("null"))
                }).collect::<Vec<_>>().join(" ");
                let expression = match self {
                    Self::Boolean(BooleanOperation::Not, _) => "!v0".into(),
                    Self::Boolean(BooleanOperation::And, _) => "v0 && v1".into(),
                    Self::Boolean(BooleanOperation::Or, _) => "v0 || v1".into(),
                    Self::Boolean(BooleanOperation::Xor, _) => "v0 !== v1".into(),
                    _ => (0..tests.len())
                        .map(|index| format!("v{index}"))
                        .collect::<Vec<_>>()
                        .join(" && "),
                };
                format!("{locals} return {expression};")
            }
            Self::Compare(a, op, b) => {
                let op = match op.as_str() {
                    "=" => "===",
                    "!=" => "!==",
                    s => s,
                };
                let integer_result = if invalid == "false" {
                    format!("a !== null && b !== null && a {op} b")
                } else {
                    format!("a !== null && b !== null ? a {op} b : {invalid}")
                };
                let float_result = if invalid == "false" {
                    format!("Number.isFinite(a) && Number.isFinite(b) && a {op} b")
                } else {
                    format!("Number.isFinite(a) && Number.isFinite(b) ? a {op} b : {invalid}")
                };
                if a.has_length() || b.has_length() {
                    format!(
                        "const n = typeof value === 'string' ? BigInt(Array.from(value).length) : value instanceof Uint8Array || Array.isArray(value) ? BigInt(value.length) : value !== null && typeof value === 'object' ? BigInt(Object.keys(value).length) : null; if (n === null) return {invalid}; const a = {}; const b = {}; return {integer_result};",
                        a.javascript(false),
                        b.javascript(false)
                    )
                } else {
                    let float = format!(
                        "if (typeof value === 'number') {{ const n = value; const a = {}; const b = {}; return {float_result}; }}",
                        a.javascript(true),
                        b.javascript(true)
                    );
                    let integer = if a.has_float() || b.has_float() {
                        format!("return {invalid};")
                    } else {
                        format!(
                            "if (typeof value !== 'bigint') return {invalid}; const n = value; const a = {}; const b = {}; return {integer_result};",
                            a.javascript(false),
                            b.javascript(false)
                        )
                    };
                    format!("{float} {integer}")
                }
            }
        }
    }
}
fn quote(text: &str) -> String {
    serde_json::to_string(text).expect("text is JSON")
}
