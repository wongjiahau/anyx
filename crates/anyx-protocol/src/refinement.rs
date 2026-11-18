//! Checked predicates shared by type refinements and method conditions.
//! No predicate evaluates application code or applies operator precedence.
use crate::Value;
use crate::schema::{Diagnostic, Schema, Type, TypeKind};
use anyx::{NamePart, Node, NodeKind, Span};
use std::borrow::Cow;

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
    Text(B, String, String),
}

impl Predicate {
    pub(crate) fn parse(nodes: &[Node], source: &str) -> Result<Self, String> {
        let mut predicate = Self::from_nodes(nodes)?;
        predicate.source = source.trim().to_string();
        Ok(predicate)
    }

    pub(crate) fn from_nodes(nodes: &[Node]) -> Result<Self, String> {
        Self::read(
            nodes,
            &|node| {
                if matches!(node.kind, NodeKind::Underscore) {
                    Ok(())
                } else {
                    Err(unsupported())
                }
            },
            false,
        )
    }

    pub fn words(&self) -> String {
        self.describe(&|_| "_".into())
    }

    pub(crate) fn check_type(&self, schema: &Schema, base: &Type) -> Result<(), String> {
        let base = schema.underlying(base);
        for test in &self.tests {
            match test {
                Test::Text(..) if base != &Type::Text => {
                    return Err("text tests require Text".into());
                }
                Test::Compare(a, _, b) => {
                    for expression in [a, b] {
                        if expression.has_value() && !matches!(base, Type::Integer | Type::Float) {
                            return Err("numeric `_` requires Integer or Float; use `(_ length)` to compare a length".into());
                        }
                        if expression.has_length()
                            && !matches!(
                                base,
                                Type::Text | Type::Bytes | Type::List(_) | Type::Map(_)
                            )
                        {
                            return Err("`_ length` requires Text, Bytes, LIST or MAP".into());
                        }
                        if expression.has_float() && base != &Type::Float {
                            return Err("fractional constants require Float; use whole-number constants for integers and lengths".into());
                        }
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn failure(&self, value: &Value) -> Option<String> {
        self.tests
            .iter()
            .find(|test| !test.accepts(&|_| Some(value), matches!(value, Value::Float(_))))
            .map(Test::words)
    }

    /// The comparisons and text tests that must all hold.
    pub fn tests(&self) -> &[Test] {
        &self.tests
    }
}

impl<B: Clone> Predicate<B> {
    fn read(
        nodes: &[Node],
        binding: &impl Fn(&Node) -> Result<B, String>,
        method: bool,
    ) -> Result<Self, String> {
        if nodes.is_empty() {
            return Err("expected at least one predicate inside brackets".into());
        }
        let mut tests = Vec::new();
        for node in nodes {
            let NodeKind::Call(call) = &node.kind else {
                return Err(unsupported());
            };
            if let Some(test) = text_test(call, binding, method)? {
                tests.push(test);
                continue;
            }
            let mut operands = Vec::new();
            let mut operators = Vec::new();
            let mut start = 0;
            for (index, part) in call.name.iter().enumerate() {
                if let NamePart::Word { text, .. } = part
                    && ["<", "<=", ">", ">=", "=", "!="].contains(&text.as_str())
                {
                    operands.push(operand(call, start, index, binding, method)?);
                    operators.push(text);
                    start = index + 1;
                }
            }
            if operators.is_empty() {
                return Err(unsupported());
            }
            operands.push(operand(call, start, call.name.len(), binding, method)?);
            tests.extend(
                operands
                    .windows(2)
                    .zip(operators)
                    .map(|(pair, op)| Test::Compare(pair[0].clone(), op.clone(), pair[1].clone())),
            );
        }
        if !tests.iter().any(Test::uses_value) {
            return Err(if method {
                "expected a predicate that refers to a method argument or result"
            } else {
                "found a predicate that never uses `_`; expected a predicate of the value"
            }
            .into());
        }
        Ok(Self {
            source: nodes.iter().map(node_text).collect::<Vec<_>>().join(", "),
            tests,
            path: String::new(),
            span: Span::new(nodes[0].span.start, nodes.last().unwrap().span.end),
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

fn text_test<B>(
    call: &anyx::Call,
    binding: &impl Fn(&Node) -> Result<B, String>,
    method: bool,
) -> Result<Option<Test<B>>, String> {
    let Some(Node {
        kind: NodeKind::String(text),
        ..
    }) = call.arguments.last()
    else {
        return Ok(None);
    };
    for op in ["starts with", "ends with", "contains"] {
        let words: Vec<_> = op.split(' ').collect();
        let Some(end) = call.name.len().checked_sub(words.len() + 1) else {
            continue;
        };
        if matches!(call.name.last(), Some(NamePart::Slot))
            && call.name[end..call.name.len() - 1]
                .iter()
                .zip(words)
                .all(|(part, word)| matches!(part, NamePart::Word { text, .. } if text == word))
        {
            let node = subexpression(call, 0, end, method)?;
            return Ok(Some(Test::Text(
                binding(&node)?,
                op.into(),
                text.value.clone(),
            )));
        }
    }
    Ok(None)
}

fn operand<B>(
    call: &anyx::Call,
    start: usize,
    end: usize,
    binding: &impl Fn(&Node) -> Result<B, String>,
    method: bool,
) -> Result<Expr<B>, String> {
    expr(
        subexpression(call, start, end, method)?.as_ref(),
        binding,
        method,
    )
}

fn subexpression(
    call: &anyx::Call,
    start: usize,
    end: usize,
    method: bool,
) -> Result<Cow<'_, Node>, String> {
    let parts = &call.name[start..end];
    let slot = |part: &&NamePart| matches!(part, NamePart::Slot);
    let first = call.name[..start].iter().filter(slot).count();
    if matches!(parts, [NamePart::Slot]) {
        return Ok(Cow::Borrowed(&call.arguments[first]));
    }
    if !method || parts.is_empty() {
        return Err(unsupported());
    }
    let count = parts.iter().filter(slot).count();
    // Between comparison symbols, named values and arithmetic can be
    // part of the outer call's name. Read those parts as one operand.
    Ok(Cow::Owned(Node {
        kind: NodeKind::Call(anyx::Call {
            name: parts.to_vec(),
            arguments: call.arguments[first..first + count].to_vec(),
        }),
        span: Span::empty(0),
    }))
}

impl Predicate<usize> {
    pub(crate) fn method(
        nodes: &[Node],
        source: &str,
        parameters: &[crate::schema::Parameter],
        returns: Option<&crate::schema::Parameter>,
    ) -> Result<Self, String> {
        let mut predicate = Self::read(
            nodes,
            &|node| {
                let NodeKind::Call(call) = &node.kind else {
                    return Err("expected a named method argument or result".into());
                };
                if !call.arguments.is_empty() {
                    return Err(unsupported());
                }
                let name = call.name_text();
                let mut matches = parameters
                    .iter()
                    .chain(returns)
                    .enumerate()
                    .filter(|(_, parameter)| parameter.name == name);
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
        let ty = |index: usize| schema.underlying(&parameters[index].r#type);
        for test in &self.tests {
            match test {
                Test::Text(index, ..) if ty(*index) != &Type::Text => {
                    return Err(format!(
                        "text tests require Text, found `{}`",
                        parameters[*index].name
                    ));
                }
                Test::Compare(a, _, b) => {
                    let invalid_numeric = |expr: &Expr<usize>| matches!(expr, Expr::Value(index) if !matches!(ty(*index), Type::Integer | Type::Float));
                    if a.any(invalid_numeric) || b.any(invalid_numeric) {
                        return Err("numeric comparisons require Integer or Float values".into());
                    }
                    let invalid_length = |expr: &Expr<usize>| matches!(expr, Expr::Length(index) if !matches!(ty(*index), Type::Text | Type::Bytes | Type::List(_) | Type::Map(_)));
                    if a.any(invalid_length) || b.any(invalid_length) {
                        return Err("length requires Text, Bytes, LIST or MAP".into());
                    }
                    let float = a.has_float()
                        || b.has_float()
                        || test.references(|index| ty(*index) == &Type::Float);
                    let integer = |expr: &Expr<usize>| {
                        matches!(expr, Expr::Length(_))
                            || matches!(expr, Expr::Value(index) if ty(*index) == &Type::Integer)
                    };
                    if float && (a.any(integer) || b.any(integer)) {
                        return Err("use whole-number constants with Integer values and lengths, or compare Float values separately".into());
                    }
                }
                _ => {}
            }
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
            let float = matches!(test, Test::Compare(a, _, b) if
                a.has_float() || b.has_float() ||
                test.references(|index| matches!(value(index), Some(Value::Float(_)))));
            if !test.accepts(&value, float) {
                let sent = parameters
                    .iter()
                    .enumerate()
                    .filter(|(index, _)| test.references(|used| used == index))
                    .filter_map(|(index, parameter)| {
                        value(&index).map(|value| {
                            format!(
                                "`{}` ({}) = `{}`",
                                parameter.name,
                                schema.type_name(&parameter.r#type),
                                value_text(value)
                            )
                        })
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                return Err(format!(
                    "{clause} [{}] failed for `{}` with {sent}; expected {}",
                    self.source,
                    schema.typed_shape(&method.shape),
                    test.describe(&name),
                ));
            }
        }
        Ok(())
    }
}

impl<B> Test<B> {
    fn references(&self, matches: impl Fn(&B) -> bool + Copy) -> bool {
        let uses = |expr: &Expr<B>| match expr {
            Expr::Value(index) | Expr::Length(index) => matches(index),
            _ => false,
        };
        match self {
            Self::Text(index, ..) => matches(index),
            Self::Compare(a, _, b) => a.any(uses) || b.any(uses),
        }
    }
}

pub(crate) fn check_precondition(
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
    "unsupported predicate; expected a comparison chain such as `10 <= _ < 30`, a text test such as `_ ends with \".poentaro\"`, or a comparison of `(_ length)`; put arithmetic in parentheses, as in `(_ + 1) <= 30`".into()
}

fn expr<B>(
    node: &Node,
    binding: &impl Fn(&Node) -> Result<B, String>,
    method: bool,
) -> Result<Expr<B>, String> {
    Ok(match &node.kind {
        NodeKind::Parens(items) if items.len() == 1 => return expr(&items[0], binding, method),
        NodeKind::Number(text) => {
            if let Ok(n) = text.parse::<i64>() {
                Expr::Integer(n)
            } else {
                let n = text.parse::<f64>().map_err(|_| unsupported())?;
                if !n.is_finite() {
                    return Err(unsupported());
                }
                Expr::Float(n)
            }
        }
        NodeKind::Call(call) => {
            if method
                && call.arguments.is_empty()
                && let Ok(value) = binding(node)
            {
                return Ok(Expr::Value(value));
            }
            let parts = &call.name;
            if let Some(NamePart::Word { text, .. }) = parts.last()
                && text == "length"
            {
                let subject = subexpression(call, 0, parts.len() - 1, method)?;
                return Ok(Expr::Length(binding(&subject)?));
            }
            if let Some((index, op)) = parts.iter().enumerate().find_map(|(i, part)| match part {
                NamePart::Word { text, .. } if ["+", "-", "*"].contains(&text.as_str()) => {
                    Some((i, text.as_str()))
                }
                _ => None,
            }) {
                if parts[index + 1..].iter().any(|part| matches!(part, NamePart::Word { text, .. } if ["+", "-", "*"].contains(&text.as_str()))) {
                    return Err(unsupported());
                }
                let a = Box::new(operand(call, 0, index, binding, method)?);
                let b = Box::new(operand(call, index + 1, parts.len(), binding, method)?);
                return Ok(match op {
                    "+" => Expr::Add(a, b),
                    "-" => Expr::Sub(a, b),
                    "*" if a.constant() || b.constant() => Expr::Mul(a, b),
                    _ => return Err(unsupported()),
                });
            }
            Expr::Value(binding(node)?)
        }
        _ => Expr::Value(binding(node)?),
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
impl<B> Test<B> {
    fn uses_value(&self) -> bool {
        match self {
            Self::Text(..) => true,
            Self::Compare(a, _, b) => !a.constant() || !b.constant(),
        }
    }
    fn accepts<'a>(&self, value: &impl Fn(&B) -> Option<&'a Value>, float: bool) -> bool {
        match self {
            Self::Text(binding, op, text) => {
                if let Some(Value::Text(s)) = value(binding) {
                    match op.as_str() {
                        "starts with" => s.starts_with(text),
                        "ends with" => s.ends_with(text),
                        "contains" => s.contains(text),
                        _ => false,
                    }
                } else {
                    false
                }
            }
            Self::Compare(a, op, b) => {
                if float {
                    a.float(value)
                        .zip(b.float(value))
                        .is_some_and(|(a, b)| compare(a, op, b))
                } else {
                    a.integer(value)
                        .zip(b.integer(value))
                        .is_some_and(|(a, b)| compare(a, op, b))
                }
            }
        }
    }
    fn describe(&self, name: &impl Fn(&B) -> String) -> String {
        match self {
            Self::Text(binding, op, text) => {
                let subject = name(binding);
                if subject == "_" {
                    format!("{op} {text:?}")
                } else {
                    format!("{subject} {op} {text:?}")
                }
            }
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

fn node_text(node: &Node) -> String {
    match &node.kind {
        NodeKind::Underscore => "_".into(),
        NodeKind::Number(s) => s.clone(),
        NodeKind::String(s) => quote(&s.value),
        NodeKind::Parens(nodes) => format!(
            "({})",
            nodes.iter().map(node_text).collect::<Vec<_>>().join(", ")
        ),
        NodeKind::Call(call) => {
            let mut args = call.arguments.iter();
            call.name
                .iter()
                .map(|part| match part {
                    NamePart::Word { text, .. } => text.clone(),
                    NamePart::Slot => {
                        let arg = args.next().unwrap();
                        let text = node_text(arg);
                        if matches!(arg.kind, NodeKind::Call(_)) {
                            format!("({text})")
                        } else {
                            text
                        }
                    }
                })
                .collect::<Vec<_>>()
                .join(" ")
        }
        _ => "…".into(),
    }
}

/// Checks a value's shape and all refinements, including nested fields and items.
/// Used by decoders, encoders, and generated checked constructors.
pub fn validate(schema: &Schema, expected: &Type, value: &Value) -> Result<(), String> {
    fn walk(schema: &Schema, ty: &Type, value: &Value, label: &str) -> Result<(), String> {
        match ty {
            Type::Refined(base, predicate) => {
                walk(schema, base, value, label)?;
                if let Some(failed) = predicate.failure(value) {
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
                // instance reads past distinct types, so preserve them explicitly first.
                if let Type::Named(index) = ty
                    && let TypeKind::Distinct(base) = &schema.types[*index].kind
                {
                    return walk(schema, base, value, label);
                }
                let (definition, kind) = schema.instance(ty).ok_or("expected a known type")?;
                match (kind.as_ref(), value) {
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
                    (TypeKind::Choice(variants), Value::Variant { shape, arguments }) => {
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

pub(crate) fn check_schema(schema: &Schema) -> Vec<Diagnostic> {
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
            Type::Generic(_, args) => {
                for arg in args {
                    check(schema, arg, errors);
                }
            }
            _ => {}
        }
    }
    let mut errors = Vec::new();
    for definition in &schema.types {
        match &definition.kind {
            TypeKind::Distinct(base) => check(schema, base, &mut errors),
            TypeKind::Record(fields) => {
                for f in fields {
                    check(schema, &f.r#type, &mut errors);
                }
            }
            TypeKind::Choice(variants) => {
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
    for error in &schema.errors {
        for p in &error.error.shape.parameters {
            check(schema, &p.r#type, &mut errors);
        }
    }
    errors
}

/// Whether this type contains any predicate, including those of named types.
pub fn has_refinements(schema: &Schema, ty: &Type) -> bool {
    fn walk(schema: &Schema, ty: &Type, seen: &mut Vec<usize>) -> bool {
        match ty {
            Type::Refined(..) => true,
            Type::List(item) | Type::Map(item) => walk(schema, item, seen),
            Type::Named(index) | Type::Generic(index, _) => {
                if seen.contains(index) {
                    return false;
                }
                seen.push(*index);
                let found = match &schema.types[*index].kind {
                    TypeKind::Distinct(base) => walk(schema, base, seen),
                    TypeKind::Record(fields) => {
                        fields.iter().any(|f| walk(schema, &f.r#type, seen))
                    }
                    TypeKind::Choice(variants) => variants.iter().any(|v| {
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
                let (mut prefix, mut middle, mut suffix) =
                    (String::new(), String::new(), String::new());
                for test in &predicate.tests {
                    match test {
                        Test::Compare(a, _, b) => {
                            constants(a, &mut numbers);
                            constants(b, &mut numbers);
                        }
                        Test::Text((), op, text) => match op.as_str() {
                            "starts with" => prefix.push_str(text),
                            "ends with" => suffix.push_str(text),
                            _ => middle.push_str(text),
                        },
                    }
                }
                match schema.underlying(base) {
                    Type::Integer | Type::Float => {
                        for n in numbers {
                            for n in [n.checked_sub(1), Some(n), n.checked_add(1)]
                                .into_iter()
                                .flatten()
                            {
                                values.push(if schema.underlying(base) == &Type::Integer {
                                    Value::Integer(n)
                                } else {
                                    Value::Float(n as f64)
                                });
                            }
                        }
                        if schema.underlying(base) == &Type::Float {
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
            Type::Named(index) if matches!(schema.types[*index].kind, TypeKind::Distinct(_)) => {
                let TypeKind::Distinct(base) = &schema.types[*index].kind else {
                    unreachable!()
                };
                candidates(schema, base, depth + 1)
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
