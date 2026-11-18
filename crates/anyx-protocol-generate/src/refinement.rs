//! Native TypeScript checks emitted from the codec's checked predicate AST.
use anyx_protocol::refinement::{Expr, Predicate, Test};

pub fn checks(predicate: &Predicate) -> String {
    format!(
        "[{}]",
        predicate
            .tests()
            .iter()
            .map(|test| {
                format!(
                    "{{source:{}, words:{}, test:(value: unknown): boolean => {{ {} }} }}",
                    quote(&predicate.source),
                    quote(&test.words()),
                    test.javascript()
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
    fn javascript(&self) -> String;
}
impl TestJavascript for Test {
    fn javascript(&self) -> String {
        match self {
            Self::Text((), op, text) => format!(
                "return typeof value === 'string' && value.{}({});",
                match op.as_str() {
                    "starts with" => "startsWith",
                    "ends with" => "endsWith",
                    _ => "includes",
                },
                quote(text)
            ),
            Self::Compare(a, op, b) => {
                let op = match op.as_str() {
                    "=" => "===",
                    "!=" => "!==",
                    s => s,
                };
                if a.has_length() || b.has_length() {
                    format!(
                        "const n = typeof value === 'string' ? BigInt(Array.from(value).length) : value instanceof Uint8Array || Array.isArray(value) ? BigInt(value.length) : value !== null && typeof value === 'object' ? BigInt(Object.keys(value).length) : null; if (n === null) return false; const a = {}; const b = {}; return a !== null && b !== null && a {op} b;",
                        a.javascript(false),
                        b.javascript(false)
                    )
                } else {
                    let float = format!(
                        "if (typeof value === 'number') {{ const n = value; const a = {}; const b = {}; return Number.isFinite(a) && Number.isFinite(b) && a {op} b; }}",
                        a.javascript(true),
                        b.javascript(true)
                    );
                    let integer = if a.has_float() || b.has_float() {
                        "return false;".into()
                    } else {
                        format!(
                            "if (typeof value !== 'bigint') return false; const n = value; const a = {}; const b = {}; return a !== null && b !== null && a {op} b;",
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
