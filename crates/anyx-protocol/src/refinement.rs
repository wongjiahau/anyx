//! Checked predicates shared by type refinements and method conditions.
//! No predicate evaluates application code or applies operator precedence.
use crate::Value;
use crate::grammar::{Term, TermVariant};
use crate::prelude::{self, Arithmetic, BooleanOperation, Function};
use crate::schema::{Diagnostic, Schema, ShapePart, Type, TypeKind};
use anyx::{NamePart, Span};

#[derive(Clone, Debug)]
pub struct Predicate<B = ()> {
    /// The predicate inside its brackets, as written in the schema.
    pub source: String,
    tests: Vec<Test<B>>,
    span: Span,
    pub(crate) path: String,
}

impl<B: PartialEq> PartialEq for Predicate<B> {
    fn eq(&self, other: &Self) -> bool {
        self.tests == other.tests
    }
}

#[derive(Clone, Debug, PartialEq)]
/// A numeric expression in a checked predicate.
pub enum Expr<B = ()> {
    Value(B),
    Length(B),
    Integer(i64),
    Float(f64),
    Add(Box<Expr<B>>, Box<Expr<B>>),
    Sub(Box<Expr<B>>, Box<Expr<B>>),
    Mul(Box<Expr<B>>, Box<Expr<B>>),
}
#[derive(Clone, Debug, PartialEq)]
/// One comparison or text test of a checked predicate.
pub enum Test<B = ()> {
    Compare(Expr<B>, String, Expr<B>),
    Text(TextOperand<B>, &'static str, TextOperand<B>),
    Boolean(BooleanOperation, Vec<Test<B>>),
    All(String, Vec<Test<B>>),
    Flag(B),
    Literal(bool),
    /// `items . has no repeats`: no two items of the list are equal.
    NoRepeats(B),
    /// `items . has at most one of [a, b]`: at most one item of the list is
    /// a variant of these shapes, written with `_` in each slot.
    AtMostOneOf(B, Vec<String>),
}

#[derive(Clone, Debug, PartialEq)]
pub enum TextOperand<B> {
    Value(B),
    Literal(String),
}

impl Predicate {
    pub(crate) fn parse(terms: &[Term], source: &str) -> Result<Self, String> {
        let mut predicate = Self::from_terms(terms)?;
        predicate.source = source.trim().to_string();
        Ok(predicate)
    }

    pub(crate) fn from_terms(terms: &[Term]) -> Result<Self, String> {
        Self::read(
            terms,
            &|term| match term {
                Term::Subject(_) => Ok(()),
                _ => Err(unsupported()),
            },
            false,
        )
    }

    pub fn words(&self) -> String {
        self.describe(&|_| "_".into())
    }

    pub(crate) fn check_type(&self, schema: &Schema, base: &Type) -> Result<(), String> {
        let base = schema.held_type(base);
        for test in &self.tests {
            if base != &Type::Float && test.has_fractional_constant() {
                return Err("fractional constants require Float; use whole-number constants for integers and lengths".into());
            }
            test.check_types(&|_| base, schema).map_err(|message| {
                if message == "numeric comparisons require Integer or Float values" {
                    "numeric `_` requires Integer or Float; use `(_ length)` to compare a length"
                        .into()
                } else {
                    message
                }
            })?;
        }
        Ok(())
    }

    fn failure(&self, value: &Value) -> Option<String> {
        self.tests
            .iter()
            .find_map(|test| test.failure(&|_| Some(value)))
            .map(Test::words)
    }

    /// The comparisons and text tests that must all hold.
    pub fn tests(&self) -> &[Test] {
        &self.tests
    }
}

impl<B: Clone> Predicate<B> {
    fn read(
        terms: &[Term],
        binding: &impl Fn(&Term) -> Result<B, String>,
        method: bool,
    ) -> Result<Self, String> {
        let (Some(first), Some(last)) = (terms.first(), terms.last()) else {
            return Err("expected at least one predicate inside brackets".into());
        };
        let tests = terms
            .iter()
            .map(|term| Test::read(term, binding, method))
            .collect::<Result<Vec<_>, _>>()?;
        if !tests.iter().any(Test::uses_value) {
            return Err(if method {
                "expected a predicate that refers to a method argument or result"
            } else {
                "found a predicate that never uses `_`; expected a predicate of the value"
            }
            .into());
        }
        Ok(Self {
            source: terms.iter().map(term_text).collect::<Vec<_>>().join(", "),
            tests,
            path: String::new(),
            span: Span::new(first.span().start, last.span().end),
        })
    }

    fn describe(&self, name: &impl Fn(&B) -> String) -> String {
        self.tests
            .iter()
            .map(|test| test.describe(name))
            .collect::<Vec<_>>()
            .join(" and ")
    }
}

impl Predicate<usize> {
    pub(crate) fn method(
        terms: &[Term],
        source: &str,
        parameters: &[crate::schema::Parameter],
        returns: Option<&crate::schema::Parameter>,
    ) -> Result<Self, String> {
        let mut predicate = Self::read(
            terms,
            &|term| {
                let Term::Call {
                    name, arguments, ..
                } = term
                else {
                    return Err("expected a named method argument or result".into());
                };
                if !arguments.is_empty() {
                    return Err(unsupported());
                }
                let name = &name.text;
                let mut matches = parameters
                    .iter()
                    .chain(returns)
                    .enumerate()
                    .filter(|(_, parameter)| parameter.name == *name);
                let (index, _) = matches.next().ok_or_else(|| {
                    format!(
                        "unknown value `{name}`; use only the method's arguments{}",
                        if returns.is_some() {
                            " and its result"
                        } else {
                            ""
                        }
                    )
                })?;
                if matches.next().is_some() {
                    return Err(format!(
                        "ambiguous value `{name}`; give the arguments and result distinct names"
                    ));
                }
                Ok(index)
            },
            true,
        )?;
        predicate.source = source.trim().to_string();
        Ok(predicate)
    }

    fn check_method_types(
        &self,
        schema: &Schema,
        parameters: &[&crate::schema::Parameter],
    ) -> Result<(), String> {
        let ty = |index: &usize| schema.held_type(&parameters[*index].r#type);
        for test in &self.tests {
            test.check_types(&ty, schema)?;
        }
        Ok(())
    }

    fn check_method_values(
        &self,
        schema: &Schema,
        method: &crate::schema::Method,
        arguments: &[Value],
        result: Option<&Value>,
        clause: &str,
    ) -> Result<(), String> {
        let parameters: Vec<_> = method
            .shape
            .parameters
            .iter()
            .chain(method.returns.as_ref().filter(|_| result.is_some()))
            .collect();
        let value = |index: &usize| {
            if *index == method.shape.parameters.len() {
                result
            } else {
                arguments.get(*index)
            }
        };
        let name = |index: &usize| parameters[*index].name.clone();
        for test in &self.tests {
            if let Some(failed) = test.failure(&value) {
                let sent = parameters
                    .iter()
                    .enumerate()
                    .filter(|(index, _)| failed.references(|used| used == index))
                    .filter_map(|(index, parameter)| {
                        value(&index).map(|value| {
                            let written =
                                crate::codec::encode_value(schema, &parameter.r#type, value)
                                    .map_or_else(
                                        |_| value_text(value),
                                        |text| text.trim_end().to_string(),
                                    );
                            format!(
                                "`{}` ({}) = {}",
                                parameter.name,
                                schema.type_name(&parameter.r#type),
                                crate::schema::code(&written),
                            )
                        })
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                // Quote the whole condition only when it is one test; the
                // failed one is named after `expected` either way.
                let quoted = match self.tests.len() {
                    1 => format!(" [{}]", self.source),
                    _ => String::new(),
                };
                return Err(format!(
                    "{clause}{quoted} failed for `{}` with {sent}; expected {}",
                    schema.typed_shape(&method.shape),
                    failed.describe(&name),
                ));
            }
        }
        Ok(())
    }
}

/// Checks `method`'s `REQUIRES` against a call's `arguments`, as the decoder
/// does before dispatch. A client that builds calls from values it computed,
/// such as the anyx shell, checks them with it before sending.
pub fn check_precondition(
    schema: &Schema,
    method: &crate::schema::Method,
    arguments: &[Value],
) -> Result<(), String> {
    if let Some(predicate) = &method.requires {
        predicate.check_method_values(schema, method, arguments, None, "REQUIRES")?;
    }
    Ok(())
}

pub(crate) fn check_postcondition(
    schema: &Schema,
    method: &crate::schema::Method,
    arguments: &[Value],
    outcome: &crate::Outcome,
) -> Result<(), String> {
    if let (Some(predicate), crate::Outcome::Value(value)) = (&method.ensures, outcome) {
        predicate.check_method_values(schema, method, arguments, Some(value), "ENSURES")?;
    }
    Ok(())
}

pub(crate) fn check_methods(schema: &Schema) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    for method in &schema.methods {
        for (clause, predicate, returns) in [
            ("REQUIRES", &method.requires, None),
            ("ENSURES", &method.ensures, method.returns.as_ref()),
        ] {
            if let Some(predicate) = predicate {
                let parameters: Vec<_> = method.shape.parameters.iter().chain(returns).collect();
                if let Err(message) = predicate.check_method_types(schema, &parameters) {
                    diagnostics.push(Diagnostic {
                        path: predicate.path.clone(),
                        span: predicate.span,
                        message: format!("invalid {clause} [{}]: {message}", predicate.source),
                    });
                }
            }
        }
    }
    diagnostics
}

fn unsupported() -> String {
    "unsupported predicate; expected a comparison chain such as `10 <= _ < 30`, a text test such as `_ ends with \".poentaro\"`, or a comparison of `(_ length)`; group nested calls with parentheses, dot, or colon, as in `(_ + 1) <= 30` or `30 >=: _ + 1`".into()
}

fn expr<B>(term: &Term, binding: &impl Fn(&Term) -> Result<B, String>) -> Result<Expr<B>, String> {
    Ok(match term {
        Term::Integer(n) => Expr::Integer(n.value.parse().map_err(|_| unsupported())?),
        Term::Float(n) => {
            let n = n.value.parse::<f64>().map_err(|_| unsupported())?;
            if !n.is_finite() {
                return Err(unsupported());
            }
            Expr::Float(n)
        }
        Term::Call {
            name, arguments, ..
        } => match (prelude::function(&name.text), arguments.as_slice()) {
            (Some(Function::Length), [subject]) => Expr::Length(binding(subject)?),
            (Some(Function::Arithmetic(op)), [left, right]) => {
                let a = Box::new(expr(left, binding)?);
                let b = Box::new(expr(right, binding)?);
                match op {
                    Arithmetic::Add => Expr::Add(a, b),
                    Arithmetic::Subtract => Expr::Sub(a, b),
                    Arithmetic::Multiply if a.constant() || b.constant() => Expr::Mul(a, b),
                    _ => return Err(unsupported()),
                }
            }
            (None, []) => Expr::Value(binding(term)?),
            _ => return Err(unsupported()),
        },
        _ => Expr::Value(binding(term)?),
    })
}

impl<B> Expr<B> {
    fn any(&self, f: impl Fn(&Self) -> bool + Copy) -> bool {
        f(self)
            || match self {
                Self::Add(a, b) | Self::Sub(a, b) | Self::Mul(a, b) => a.any(f) || b.any(f),
                _ => false,
            }
    }
    fn has_value(&self) -> bool {
        self.any(|e| matches!(e, Self::Value(_)))
    }
    pub fn has_length(&self) -> bool {
        self.any(|e| matches!(e, Self::Length(_)))
    }
    pub fn has_float(&self) -> bool {
        self.any(|e| matches!(e, Self::Float(_)))
    }
    fn constant(&self) -> bool {
        !self.has_value() && !self.has_length()
    }
    fn check_numeric<'a>(&self, ty: &impl Fn(&B) -> &'a Type, numeric: &str) -> Result<(), String> {
        match self {
            Self::Value(index) if !matches!(ty(index), Type::Integer | Type::Float) => {
                return Err("numeric comparisons require Integer or Float values".into());
            }
            Self::Value(index) if matches!(ty(index), Type::Integer) && numeric == "Float" => {
                return Err("use whole-number constants with Integer values and lengths, or compare Float values separately".into());
            }
            Self::Length(index) => {
                let input = match ty(index) {
                    Type::Text => "Text",
                    Type::Bytes => "Bytes",
                    Type::List(_) => "LIST Type",
                    Type::Map(_) => "MAP Type",
                    _ => return Err("length requires Text, Bytes, LIST or MAP".into()),
                };
                if numeric != "Integer" {
                    return Err("use whole-number constants with Integer values and lengths, or compare Float values separately".into());
                }
                if !prelude::function_takes("_ length", &[input]) {
                    return Err("no prelude length overload for this operand".into());
                }
            }
            Self::Add(a, b) | Self::Sub(a, b) | Self::Mul(a, b) => {
                let shape = match self {
                    Self::Add(..) => "_ + _",
                    Self::Sub(..) => "_ - _",
                    _ => "_ * _",
                };
                if !prelude::function_takes(shape, &[numeric, numeric]) {
                    return Err("no prelude arithmetic overload for these operands".into());
                }
                a.check_numeric(ty, numeric)?;
                b.check_numeric(ty, numeric)?;
            }
            _ => {}
        }
        Ok(())
    }
    fn integer<'a>(&self, value: &impl Fn(&B) -> Option<&'a Value>) -> Option<i128> {
        match self {
            Self::Value(binding) => {
                if let Value::Integer(n) = value(binding)? {
                    Some(*n as i128)
                } else {
                    None
                }
            }
            Self::Length(binding) => length(value(binding)?).map(|n| n as i128),
            Self::Integer(n) => Some(*n as i128),
            Self::Float(_) => None,
            Self::Add(a, b) => a.integer(value)?.checked_add(b.integer(value)?),
            Self::Sub(a, b) => a.integer(value)?.checked_sub(b.integer(value)?),
            Self::Mul(a, b) => a.integer(value)?.checked_mul(b.integer(value)?),
        }
    }
    fn float<'a>(&self, value: &impl Fn(&B) -> Option<&'a Value>) -> Option<f64> {
        let n = match self {
            Self::Value(binding) => {
                if let Value::Float(n) = value(binding)? {
                    *n
                } else {
                    return None;
                }
            }
            Self::Length(binding) => length(value(binding)?)? as f64,
            Self::Integer(n) => *n as f64,
            Self::Float(n) => *n,
            Self::Add(a, b) => a.float(value)? + b.float(value)?,
            Self::Sub(a, b) => a.float(value)? - b.float(value)?,
            Self::Mul(a, b) => a.float(value)? * b.float(value)?,
        };
        n.is_finite().then_some(n)
    }
    fn text(&self, name: &impl Fn(&B) -> String) -> String {
        match self {
            Self::Value(binding) => name(binding),
            Self::Length(binding) => format!("({} length)", name(binding)),
            Self::Integer(n) => n.to_string(),
            Self::Float(n) => format!("{n:?}"),
            Self::Add(a, b) => format!("({} + {})", a.text(name), b.text(name)),
            Self::Sub(a, b) => format!("({} - {})", a.text(name), b.text(name)),
            Self::Mul(a, b) => format!("({} * {})", a.text(name), b.text(name)),
        }
    }
}
fn length(value: &Value) -> Option<usize> {
    match value {
        Value::Text(s) => Some(s.chars().count()),
        Value::Bytes(v) => Some(v.len()),
        Value::List(v) => Some(v.len()),
        Value::Map(v) => Some(v.len()),
        _ => None,
    }
}
fn compare<T: PartialOrd>(a: T, op: &str, b: T) -> bool {
    match op {
        "<" => a < b,
        "<=" => a <= b,
        ">" => a > b,
        ">=" => a >= b,
        "=" => a == b,
        "!=" => a != b,
        _ => false,
    }
}
impl<B: Clone> Test<B> {
    fn read(
        term: &Term,
        binding: &impl Fn(&Term) -> Result<B, String>,
        named: bool,
    ) -> Result<Self, String> {
        let (shape, args) = match term {
            Term::Call {
                name, arguments, ..
            } => (name.text.clone(), arguments.as_slice()),
            Term::Variant(variant) => {
                return match variant.body.name.text.as_str() {
                    "true" => Ok(Self::Literal(true)),
                    "false" => Ok(Self::Literal(false)),
                    shape => Err(format!(
                        "found the variant `` `{shape} ``; expected a Boolean, `` `true `` or `` `false ``"
                    )),
                };
            }
            _ => return Ok(Self::Flag(binding(term)?)),
        };
        Ok(match (prelude::function(&shape), args) {
            (Some(Function::Compare(operators)), args) => {
                let expressions = args
                    .iter()
                    .map(|arg| expr(arg, binding))
                    .collect::<Result<Vec<_>, _>>()?;
                let mut comparisons = expressions.windows(2).zip(operators).map(|(pair, op)| {
                    Self::Compare(pair[0].clone(), (*op).into(), pair[1].clone())
                });
                let first = comparisons
                    .next()
                    .expect("a comparison takes at least two operands");
                if operators.len() == 1 {
                    first
                } else {
                    Self::All(shape, std::iter::once(first).chain(comparisons).collect())
                }
            }
            (Some(Function::Text(op)), [subject, pattern]) => Self::Text(
                TextOperand::read(subject, binding)?,
                op,
                TextOperand::read(pattern, binding)?,
            ),
            (Some(Function::Boolean(op)), args) => Self::Boolean(
                op,
                args.iter()
                    .map(|arg| Self::read(arg, binding, named))
                    .collect::<Result<Vec<_>, _>>()?,
            ),
            (Some(Function::NoRepeats | Function::AtMostOneOf), _) if !named => {
                return Err("list tests such as `has no repeats` are for REQUIRES and ENSURES; a refinement tests one value".into());
            }
            (Some(Function::NoRepeats), [items]) => Self::NoRepeats(binding(items)?),
            (Some(Function::AtMostOneOf), [items, variants]) => {
                Self::AtMostOneOf(binding(items)?, variant_shapes(variants)?)
            }
            (Some(_), _) => return Err("a predicate must return Boolean".into()),
            (None, []) if matches!(shape.as_str(), "true" | "false") => {
                return Err(format!(
                    "`{shape}` is not a function; did you mean the variant `` `{shape} ``?"
                ));
            }
            (None, []) if named => Self::Flag(binding(term)?),
            _ => return Err(unsupported()),
        })
    }
}

/// The shapes of `` [`any case, `_ encoding] ``: each a variant's shape,
/// with `_` in each of its slots.
fn variant_shapes(term: &Term) -> Result<Vec<String>, String> {
    match term {
        Term::Variants(variants) if variants.value.is_empty() => {
            Err("expected at least one variant in the square brackets".into())
        }
        Term::Variants(variants) => Ok(variants
            .value
            .iter()
            .map(|variant| variant.body.name.text.clone())
            .collect()),
        _ => Err(format!(
            "found `{}`; expected variants in square brackets, as in `` [`any case, `_ encoding] ``",
            term_text(term)
        )),
    }
}

impl<B> TextOperand<B> {
    fn read(term: &Term, binding: &impl Fn(&Term) -> Result<B, String>) -> Result<Self, String> {
        match term {
            Term::Text(text) => Ok(Self::Literal(text.value.value.clone())),
            _ => binding(term).map(Self::Value),
        }
    }

    fn describe(&self, name: &impl Fn(&B) -> String) -> String {
        match self {
            Self::Literal(text) => format!("{text:?}"),
            Self::Value(binding) => name(binding),
        }
    }

    fn value<'a, 'v: 'a>(&'a self, get: &impl Fn(&B) -> Option<&'v Value>) -> Option<&'a str> {
        match self {
            Self::Literal(text) => Some(text),
            Self::Value(binding) => match get(binding)? {
                Value::Text(text) => Some(text),
                _ => None,
            },
        }
    }
}

impl<B> Test<B> {
    fn uses_value(&self) -> bool {
        self.references(|_| true)
    }

    fn references(&self, matches: impl Fn(&B) -> bool + Copy) -> bool {
        let uses = |expr: &Expr<B>| match expr {
            Expr::Value(index) | Expr::Length(index) => matches(index),
            _ => false,
        };
        match self {
            Self::Compare(a, _, b) => a.any(uses) || b.any(uses),
            Self::Text(a, _, b) => [a, b]
                .iter()
                .any(|operand| matches!(operand, TextOperand::Value(index) if matches(index))),
            Self::Flag(index) | Self::NoRepeats(index) | Self::AtMostOneOf(index, _) => {
                matches(index)
            }
            Self::Boolean(_, tests) | Self::All(_, tests) => {
                tests.iter().any(|test| test.references(matches))
            }
            Self::Literal(_) => false,
        }
    }

    fn has_fractional_constant(&self) -> bool {
        match self {
            Self::Compare(a, _, b) => a.has_float() || b.has_float(),
            Self::All(_, tests) | Self::Boolean(_, tests) => {
                tests.iter().any(Self::has_fractional_constant)
            }
            _ => false,
        }
    }

    fn check_types<'a>(&self, ty: &impl Fn(&B) -> &'a Type, schema: &Schema) -> Result<(), String> {
        match self {
            Self::Text(a, _, b) => {
                if [a, b].iter().any(|operand| matches!(operand, TextOperand::Value(index) if ty(index) != &Type::Text)) {
                    return Err("text tests require Text".into());
                }
            }
            Self::Compare(a, op, b) => {
                let numeric = self.numeric_type(ty);
                a.check_numeric(ty, numeric)?;
                b.check_numeric(ty, numeric)?;
                if !prelude::function_takes(&format!("_ {op} _"), &[numeric, numeric]) {
                    return Err("no prelude comparison overload for these operands".into());
                }
            }
            Self::Flag(index) if schema.type_name(ty(index)) != "Boolean" => {
                return Err("a predicate must return Boolean".into());
            }
            Self::All(shape, tests) => {
                let numeric = self.numeric_type(ty);
                for test in tests {
                    if let Self::Compare(a, _, b) = test {
                        a.check_numeric(ty, numeric)?;
                        b.check_numeric(ty, numeric)?;
                    }
                }
                if !prelude::function_takes(shape, &vec![numeric; tests.len() + 1]) {
                    return Err("no prelude comparison overload for these operands".into());
                }
            }
            Self::Boolean(_, tests) => {
                for test in tests {
                    test.check_types(ty, schema)?;
                }
            }
            Self::NoRepeats(index) => {
                if !matches!(ty(index), Type::List(_)) {
                    return Err("`has no repeats` tests a LIST".into());
                }
            }
            Self::AtMostOneOf(index, shapes) => {
                let Type::List(item) = ty(index) else {
                    return Err("`has at most one of` tests a LIST".into());
                };
                let variants = match schema.instance(item) {
                    Some((_, kind)) => match kind.as_ref() {
                        TypeKind::Variants(variants) => variants
                            .iter()
                            .map(|variant| variant.shape.text.clone())
                            .collect::<Vec<_>>(),
                        _ => Vec::new(),
                    },
                    None => Vec::new(),
                };
                if variants.is_empty() {
                    return Err(format!(
                        "`has at most one of` tests a LIST of a union of variants, not a LIST {}",
                        schema.type_name(item)
                    ));
                }
                for shape in shapes {
                    if !variants.contains(shape) {
                        return Err(format!(
                            "`{shape}` is not a variant of {}; its variants are {}",
                            schema.type_name(item),
                            variants
                                .iter()
                                .map(|variant| format!("`{variant}`"))
                                .collect::<Vec<_>>()
                                .join(", ")
                        ));
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn numeric_type<'a>(&self, ty: &impl Fn(&B) -> &'a Type) -> &'static str {
        if self.has_fractional_constant() || self.references(|index| ty(index) == &Type::Float) {
            "Float"
        } else {
            "Integer"
        }
    }

    fn uses_float<'a>(&self, get: &impl Fn(&B) -> Option<&'a Value>) -> bool {
        self.has_fractional_constant()
            || self.references(|index| matches!(get(index), Some(Value::Float(_))))
    }

    fn evaluate_comparison<'a>(
        &self,
        get: &impl Fn(&B) -> Option<&'a Value>,
        float: bool,
    ) -> Option<bool> {
        let Self::Compare(a, op, b) = self else {
            unreachable!("comparison chain contains only comparisons")
        };
        if float {
            Some(compare(a.float(get)?, op, b.float(get)?))
        } else {
            Some(compare(a.integer(get)?, op, b.integer(get)?))
        }
    }

    fn failure<'a>(&self, value: &impl Fn(&B) -> Option<&'a Value>) -> Option<&Self> {
        match self {
            Self::All(_, tests) => {
                let float = self.uses_float(value);
                tests
                    .iter()
                    .find(|test| test.evaluate_comparison(value, float) != Some(true))
            }
            _ if self.evaluate(value) == Some(true) => None,
            _ => Some(self),
        }
    }

    fn evaluate<'a>(&self, get: &impl Fn(&B) -> Option<&'a Value>) -> Option<bool> {
        Some(match self {
            Self::Text(a, op, b) => {
                let a = a.value(get)?;
                let b = b.value(get)?;
                match *op {
                    "starts with" => a.starts_with(b),
                    "ends with" => a.ends_with(b),
                    _ => a.contains(b),
                }
            }
            Self::Compare(..) => return self.evaluate_comparison(get, self.uses_float(get)),
            Self::Flag(index) => match get(index)? {
                Value::Variant { shape, arguments } if arguments.is_empty() => {
                    shape.parse().ok()?
                }
                _ => return None,
            },
            Self::Literal(value) => *value,
            Self::NoRepeats(index) => {
                let Value::List(items) = get(index)? else {
                    return None;
                };
                items
                    .iter()
                    .enumerate()
                    .all(|(i, item)| !items[..i].contains(item))
            }
            Self::AtMostOneOf(index, shapes) => {
                let Value::List(items) = get(index)? else {
                    return None;
                };
                items
                    .iter()
                    .filter(|item| {
                        matches!(item, Value::Variant { shape, .. } if shapes.contains(shape))
                    })
                    .nth(1)
                    .is_none()
            }
            Self::Boolean(op, operands) => match operands.as_slice() {
                [a] => op.apply(a.evaluate(get)?, false),
                [a, b] => op.apply(a.evaluate(get)?, b.evaluate(get)?),
                _ => unreachable!("prelude Boolean function arity was checked"),
            },
            Self::All(_, tests) => {
                let float = self.uses_float(get);
                let mut holds = true;
                for test in tests {
                    holds &= test.evaluate_comparison(get, float)?;
                }
                holds
            }
        })
    }

    fn describe(&self, name: &impl Fn(&B) -> String) -> String {
        match self {
            Self::Text(subject, op, text) => {
                let subject = subject.describe(name);
                let text = text.describe(name);
                if subject == "_" {
                    format!("{op} {text}")
                } else {
                    format!("{subject} {op} {text}")
                }
            }
            Self::Boolean(op, operands) => match operands.as_slice() {
                [value] => format!("not ({})", value.describe(name)),
                [a, b] => format!(
                    "({}) {} ({})",
                    a.describe(name),
                    op.word(),
                    b.describe(name)
                ),
                _ => unreachable!("prelude Boolean function arity was checked"),
            },
            Self::All(_, tests) => tests
                .iter()
                .map(|test| test.describe(name))
                .collect::<Vec<_>>()
                .join(" and "),
            Self::Flag(binding) => format!("{} is true", name(binding)),
            Self::Literal(value) => format!("`{value}"),
            Self::NoRepeats(binding) => format!("{} has no repeats", name(binding)),
            Self::AtMostOneOf(binding, shapes) => format!(
                "{} has at most one of [{}]",
                name(binding),
                shapes
                    .iter()
                    .map(|shape| format!("`{shape}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Self::Compare(a, op, b) => {
                let (subject, op, bound) = if !a.constant() && b.constant() {
                    (a, op.as_str(), b)
                } else if a.constant() && !b.constant() {
                    (
                        b,
                        match op.as_str() {
                            "<" => ">",
                            "<=" => ">=",
                            ">" => "<",
                            ">=" => "<=",
                            x => x,
                        },
                        a,
                    )
                } else {
                    (a, op.as_str(), b)
                };
                let relation = match op {
                    ">=" => "at least",
                    ">" => "greater than",
                    "<=" => "at most",
                    "<" => "less than",
                    "=" => "equal to",
                    _ => "different from",
                };
                let subject = match subject {
                    Expr::Value(binding) if name(binding) == "_" => String::new(),
                    Expr::Length(binding) if name(binding) == "_" => "length ".into(),
                    e => format!("{} ", e.text(name)),
                };
                format!("{subject}{relation} {}", bound.text(name))
            }
        }
    }
}
impl Test {
    pub fn words(&self) -> String {
        self.describe(&|_| "_".into())
    }
}

/// A term as written, with its calls' arguments in parentheses.
fn term_text(term: &Term) -> String {
    match term {
        Term::Subject(_) => "_".into(),
        Term::Integer(n) | Term::Float(n) => n.value.clone(),
        Term::Text(text) => quote(&text.value.value),
        Term::Variants(variants) => format!(
            "[{}]",
            variants
                .value
                .iter()
                .map(variant_text)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Term::Variant(variant) => variant_text(variant),
        Term::Call {
            name, arguments, ..
        } => {
            let mut arguments = arguments.iter();
            name.parts
                .iter()
                .map(|part| match part {
                    NamePart::Word { text, .. } => text.clone(),
                    NamePart::Slot => {
                        let argument = arguments.next().expect("a slot has its argument");
                        let text = term_text(argument);
                        match argument {
                            Term::Call { .. } | Term::Variant(_) => format!("({text})"),
                            _ => text,
                        }
                    }
                })
                .collect::<Vec<_>>()
                .join(" ")
        }
    }
}

fn variant_text(variant: &TermVariant) -> String {
    format!("`{}", variant.body.name.text)
}

/// The value a predicate on `type` reads: past named types of one slot,
/// the value in it, as `15` in `Info 15`.
fn held<'v>(schema: &Schema, r#type: &Type, value: &'v Value) -> &'v Value {
    let mut r#type = r#type;
    let mut value = value;
    loop {
        match (schema.underlying(r#type), value) {
            (Type::Named(index), Value::Variant { arguments, .. }) => {
                match &schema.types[*index].kind {
                    TypeKind::Distinct { underlying, .. } if arguments.len() == 1 => {
                        r#type = underlying;
                        value = &arguments[0];
                    }
                    _ => return value,
                }
            }
            _ => return value,
        }
    }
}

/// Checks a value's shape and all refinements, including nested fields and items.
/// Used by decoders, encoders, and generated checked constructors.
pub fn validate(schema: &Schema, expected: &Type, value: &Value) -> Result<(), String> {
    fn walk(schema: &Schema, ty: &Type, value: &Value, label: &str) -> Result<(), String> {
        match ty {
            Type::Refined(base, predicate) => {
                walk(schema, base, value, label)?;
                if let Some(failed) = predicate.failure(held(schema, base, value)) {
                    return Err(format!(
                        "value `{}` is not a `{label}`; expected {} {} (`{}`); failed bound or test: {failed}",
                        value_text(value),
                        schema.type_name(base),
                        predicate.words(),
                        predicate.source
                    ));
                }
                Ok(())
            }
            Type::Named(_) | Type::Generic(..) => {
                // A named type's value is a call of its constructor, with a
                // value of each slot's type.
                let (definition, kind) = schema.instance(ty).ok_or("expected a known type")?;
                match (kind.as_ref(), value) {
                    (
                        TypeKind::Distinct {
                            constructor,
                            underlying,
                        },
                        Value::Variant { shape, arguments },
                    ) => {
                        let slots = crate::schema::slot_types(constructor, underlying);
                        if *shape != ShapePart::text(constructor) || slots.len() != arguments.len()
                        {
                            return Err(format!("expected a `{}`", definition.name));
                        }
                        for (slot, argument) in slots.into_iter().zip(arguments) {
                            walk(schema, slot, argument, label)?;
                        }
                        Ok(())
                    }
                    (TypeKind::Record(fields), Value::Record(values)) => {
                        for field in fields {
                            if let Some(value) = values.get(&field.name) {
                                validate(schema, &field.r#type, value)
                                    .map_err(|e| format!("in `{}`: {e}", field.name))?;
                            } else if field.default.is_none() {
                                return Err(format!(
                                    "missing field `{}` of `{}`",
                                    field.name, definition.name
                                ));
                            }
                        }
                        if values
                            .keys()
                            .any(|key| !fields.iter().any(|f| f.name == *key))
                        {
                            return Err(format!("unknown field of `{}`", definition.name));
                        }
                        Ok(())
                    }
                    (TypeKind::Variants(variants), Value::Variant { shape, arguments }) => {
                        let variant = variants
                            .iter()
                            .find(|v| v.shape.text == *shape)
                            .ok_or_else(|| format!("unknown variant `{shape}` of `{label}`"))?;
                        if variant.shape.parameters.len() != arguments.len() {
                            return Err(format!("incorrect argument count in `{shape}`"));
                        }
                        for (parameter, value) in variant.shape.parameters.iter().zip(arguments) {
                            validate(schema, &parameter.r#type, value)
                                .map_err(|e| format!("in `{}`: {e}", parameter.name))?;
                        }
                        Ok(())
                    }
                    _ => Err(format!("expected `{label}`")),
                }
            }
            Type::List(item) => {
                if let Value::List(items) = value {
                    for (i, value) in items.iter().enumerate() {
                        validate(schema, item, value)
                            .map_err(|e| format!("in item {}: {e}", i + 1))?;
                    }
                    Ok(())
                } else {
                    Err(format!("expected `{label}`"))
                }
            }
            Type::Map(item) => {
                if let Value::Map(items) = value {
                    for (key, value) in items {
                        validate(schema, item, value).map_err(|e| format!("in {key:?}: {e}"))?;
                    }
                    Ok(())
                } else {
                    Err(format!("expected `{label}`"))
                }
            }
            Type::Union(_) => match schema.member_holding(ty, value) {
                Some(member) => validate(schema, member, value),
                None => Err(format!("found `{}`; expected `{label}`", value_text(value))),
            },
            Type::Variant(own) => match value {
                Value::Variant { shape, arguments }
                    if *shape == own.text && arguments.len() == own.parameters.len() =>
                {
                    for (parameter, value) in own.parameters.iter().zip(arguments) {
                        validate(schema, &parameter.r#type, value)
                            .map_err(|e| format!("in `{}`: {e}", parameter.name))?;
                    }
                    Ok(())
                }
                _ => Err(format!("found `{}`; expected `{label}`", value_text(value))),
            },
            Type::Tuple(slots, _) => match value {
                Value::Tuple(values) if values.len() == slots.len() => {
                    for (index, (slot, value)) in slots.iter().zip(values).enumerate() {
                        validate(schema, slot, value)
                            .map_err(|e| format!("in slot {}: {e}", index + 1))?;
                    }
                    Ok(())
                }
                _ => Err(format!(
                    "expected a value for each of {} slots",
                    slots.len()
                )),
            },
            _ if matches!(
                (ty, value),
                (Type::Integer, Value::Integer(_))
                    | (Type::Text, Value::Text(_))
                    | (Type::Bytes, Value::Bytes(_))
                    | (Type::Instant, Value::Instant(_))
                    | (Type::Duration, Value::Duration(_))
            ) =>
            {
                Ok(())
            }
            // A generated generic constructor validates its concrete
            // constraints here. Its FromValue bounds check the type slots.
            Type::Parameter(_) => Ok(()),
            Type::Float if matches!(value,Value::Float(n) if n.is_finite()) => Ok(()),
            _ => Err(format!("found `{}`; expected `{label}`", value_text(value))),
        }
    }
    walk(schema, expected, value, &schema.type_name(expected))
}
fn value_text(value: &Value) -> String {
    match value {
        Value::Text(s) => format!("{s:?}"),
        Value::Integer(n) => n.to_string(),
        Value::Float(n) => format!("{n:?}"),
        _ => format!("{value:?}"),
    }
}

/// Checks each predicate in the schema's types, methods and errors, and in
/// `aliases`, the types its aliases stand for. Each is reported once,
/// however many uses reach it.
pub(crate) fn check_schema(schema: &Schema, aliases: &[&Type]) -> Vec<Diagnostic> {
    fn check(schema: &Schema, ty: &Type, errors: &mut Vec<Diagnostic>) {
        match ty {
            Type::Refined(base, predicate) => {
                if let Err(message) = predicate.check_type(schema, base) {
                    let path = predicate.path.clone();
                    errors.push(Diagnostic {
                        path,
                        span: predicate.span,
                        message: format!("invalid predicate `{}`: {message}", predicate.source),
                    });
                }
                check(schema, base, errors);
            }
            Type::List(item) | Type::Map(item) => check(schema, item, errors),
            Type::Generic(_, args) | Type::Tuple(args, _) | Type::Union(args) => {
                for arg in args {
                    check(schema, arg, errors);
                }
            }
            Type::Variant(shape) => {
                for parameter in &shape.parameters {
                    check(schema, &parameter.r#type, errors);
                }
            }
            _ => {}
        }
    }
    let mut errors = Vec::new();
    for definition in &schema.types {
        match &definition.kind {
            TypeKind::Distinct {
                underlying: base, ..
            } => check(schema, base, &mut errors),
            TypeKind::Record(fields) => {
                for f in fields {
                    check(schema, &f.r#type, &mut errors);
                }
            }
            TypeKind::Variants(variants) => {
                for v in variants {
                    for p in &v.shape.parameters {
                        check(schema, &p.r#type, &mut errors);
                    }
                }
            }
        }
    }
    for method in &schema.methods {
        for p in &method.shape.parameters {
            check(schema, &p.r#type, &mut errors);
        }
        if let Some(p) = &method.returns {
            check(schema, &p.r#type, &mut errors);
        }
    }
    for error in schema.methods.iter().flat_map(|method| &method.errors) {
        for p in &error.shape.parameters {
            check(schema, &p.r#type, &mut errors);
        }
    }
    for alias in aliases {
        check(schema, alias, &mut errors);
    }
    let mut reported: Vec<Diagnostic> = Vec::new();
    for error in errors {
        if !reported.contains(&error) {
            reported.push(error);
        }
    }
    reported
}

/// Whether this type contains any predicate, including those of named types.
pub fn has_refinements(schema: &Schema, ty: &Type) -> bool {
    fn walk(schema: &Schema, ty: &Type, seen: &mut Vec<usize>) -> bool {
        match ty {
            Type::Refined(..) => true,
            Type::List(item) | Type::Map(item) => walk(schema, item, seen),
            Type::Tuple(slots, _) | Type::Union(slots) => {
                slots.iter().any(|slot| walk(schema, slot, seen))
            }
            Type::Variant(shape) => shape
                .parameters
                .iter()
                .any(|parameter| walk(schema, &parameter.r#type, seen)),
            Type::Named(index) | Type::Generic(index, _) => {
                if seen.contains(index) {
                    return false;
                }
                seen.push(*index);
                let found = match &schema.types[*index].kind {
                    TypeKind::Distinct {
                        underlying: base, ..
                    } => walk(schema, base, seen),
                    TypeKind::Record(fields) => {
                        fields.iter().any(|f| walk(schema, &f.r#type, seen))
                    }
                    TypeKind::Variants(variants) => variants.iter().any(|v| {
                        v.shape
                            .parameters
                            .iter()
                            .any(|p| walk(schema, &p.r#type, seen))
                    }),
                };
                seen.pop();
                found
                    || matches!(ty,Type::Generic(_,args) if args.iter().any(|arg|walk(schema,arg,seen)))
            }
            _ => false,
        }
    }
    walk(schema, ty, &mut Vec::new())
}

/// Try small witnesses for guide examples. This is not a satisfiability check;
/// when no witness is found the guide uses an explicit placeholder.
pub(crate) fn example(schema: &Schema, ty: &Type) -> Option<Value> {
    fn candidates(schema: &Schema, ty: &Type, depth: usize) -> Vec<Value> {
        if depth > 16 {
            return Vec::new();
        }
        match ty {
            Type::Refined(base, predicate) => {
                let mut values = candidates(schema, base, depth + 1);
                let mut numbers = vec![0, 1, 3, 10, 20, 100];
                fn constants(e: &Expr, numbers: &mut Vec<i64>) {
                    match e {
                        Expr::Integer(n) => numbers.push(*n),
                        Expr::Add(a, b) | Expr::Sub(a, b) | Expr::Mul(a, b) => {
                            constants(a, numbers);
                            constants(b, numbers);
                        }
                        _ => {}
                    }
                }
                let mut text_parts = (String::new(), String::new(), String::new());
                fn gather(
                    test: &Test,
                    numbers: &mut Vec<i64>,
                    text_parts: &mut (String, String, String),
                ) {
                    match test {
                        Test::Compare(a, _, b) => {
                            constants(a, numbers);
                            constants(b, numbers);
                        }
                        Test::Text(TextOperand::Value(()), op, TextOperand::Literal(text)) => {
                            match *op {
                                "starts with" => text_parts.0.push_str(text),
                                "ends with" => text_parts.2.push_str(text),
                                _ => text_parts.1.push_str(text),
                            }
                        }
                        Test::All(_, tests) | Test::Boolean(_, tests) => {
                            for test in tests {
                                gather(test, numbers, text_parts);
                            }
                        }
                        _ => {}
                    }
                }
                for test in &predicate.tests {
                    gather(test, &mut numbers, &mut text_parts);
                }
                let (prefix, middle, suffix) = text_parts;
                match schema.held_type(base) {
                    ty if schema.type_name(ty) == "Boolean" => {
                        values.extend([true, false].map(crate::IntoValue::into_value));
                    }
                    Type::Integer | Type::Float => {
                        for n in numbers {
                            for n in [n.checked_sub(1), Some(n), n.checked_add(1)]
                                .into_iter()
                                .flatten()
                            {
                                values.push(if schema.held_type(base) == &Type::Integer {
                                    Value::Integer(n)
                                } else {
                                    Value::Float(n as f64)
                                });
                            }
                        }
                        if schema.held_type(base) == &Type::Float {
                            values.push(Value::Float(0.5));
                        }
                    }
                    Type::Text => {
                        values.push(Value::Text(format!("{prefix}{middle}{suffix}")));
                        for n in numbers
                            .into_iter()
                            .filter_map(|n| usize::try_from(n).ok())
                            .filter(|n| *n <= 1000)
                        {
                            values.push(Value::Text("a".repeat(n)));
                        }
                    }
                    Type::List(item) => {
                        if let Some(value) = candidates(schema, item, depth + 1).into_iter().next()
                        {
                            for n in numbers
                                .into_iter()
                                .filter_map(|n| usize::try_from(n).ok())
                                .filter(|n| *n <= 32)
                            {
                                values.push(Value::List(vec![value.clone(); n]));
                            }
                        }
                    }
                    _ => {}
                }
                values
                    .into_iter()
                    .filter(|v| validate(schema, ty, v).is_ok())
                    .collect()
            }
            Type::Named(index)
                if matches!(schema.types[*index].kind, TypeKind::Distinct { .. }) =>
            {
                let TypeKind::Distinct {
                    constructor,
                    underlying,
                } = &schema.types[*index].kind
                else {
                    unreachable!()
                };
                // Values for a named type of one slot; others are skipped.
                match crate::schema::slot_types(constructor, underlying).as_slice() {
                    [slot] => candidates(schema, slot, depth + 1)
                        .into_iter()
                        .map(|value| Value::Variant {
                            shape: ShapePart::text(constructor),
                            arguments: vec![value],
                        })
                        .collect(),
                    _ => Vec::new(),
                }
            }
            Type::Integer => vec![Value::Integer(3)],
            Type::Float => vec![Value::Float(1.5)],
            Type::Text => vec![Value::Text("a".into())],
            Type::Bytes => vec![Value::Bytes(vec![1, 2])],
            Type::List(_) => vec![Value::List(Vec::new())],
            Type::Map(_) => vec![Value::Map(Default::default())],
            _ => Vec::new(),
        }
    }
    candidates(schema, ty, 0)
        .into_iter()
        .find(|v| validate(schema, ty, v).is_ok())
}

// JSON escapes are accepted by both anyx and ES2022. Rust's Debug strings
// can emit `\\0`, which would become a legacy octal escape before a digit.
fn quote(text: &str) -> String {
    use std::fmt::Write;
    let mut out = String::from("\"");
    for c in text.chars() {
        match c {
            '\"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if c < '\u{20}' => write!(out, "\\u{:04x}", c as u32).unwrap(),
            c => out.push(c),
        }
    }
    out.push('\"');
    out
}
