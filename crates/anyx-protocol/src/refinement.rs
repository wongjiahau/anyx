//! Checked, deliberately small predicates for `_ WHERE _` type expressions.
//! No predicate evaluates application code or applies operator precedence.
use crate::Value;
use crate::schema::{Diagnostic, Schema, Type, TypeKind};
use anyx::{NamePart, Node, NodeKind, Span};

#[derive(Clone, Debug)]
pub struct Predicate {
    /// The predicate inside its brackets, as written in the schema.
    pub source: String,
    tests: Vec<Test>,
    span: Span,
    pub(crate) path: String,
}

impl PartialEq for Predicate {
    fn eq(&self, other: &Self) -> bool {
        self.tests == other.tests
    }
}

#[derive(Clone, Debug, PartialEq)]
/// A numeric expression in a checked predicate.
pub enum Expr {
    Value,
    Length,
    Integer(i64),
    Float(f64),
    Add(Box<Expr>, Box<Expr>),
    Sub(Box<Expr>, Box<Expr>),
    Mul(Box<Expr>, Box<Expr>),
}
#[derive(Clone, Debug, PartialEq)]
/// One comparison or text test of a checked predicate.
pub enum Test {
    Compare(Expr, String, Expr),
    Text(String, String),
}

impl Predicate {
    pub(crate) fn parse(nodes: &[Node], source: &str) -> Result<Self, String> {
        let mut predicate = Self::from_nodes(nodes)?;
        predicate.source = source.trim().to_string();
        Ok(predicate)
    }

    pub(crate) fn from_nodes(nodes: &[Node]) -> Result<Self, String> {
        if nodes.is_empty() {
            return Err("expected at least one predicate inside `WHERE […]`".into());
        }
        let mut tests = Vec::new();
        for node in nodes {
            let NodeKind::Call(call) = &node.kind else {
                return Err(unsupported());
            };
            let name = call.name_text();
            if ["_ starts with _", "_ ends with _", "_ contains _"].contains(&name.as_str()) {
                let [value, text] = call.arguments.as_slice() else {
                    return Err(unsupported());
                };
                let NodeKind::String(text) = &text.kind else {
                    return Err(unsupported());
                };
                if !matches!(value.kind, NodeKind::Underscore) {
                    return Err(unsupported());
                }
                tests.push(Test::Text(
                    name[2..name.len() - 2].to_string(),
                    text.value.clone(),
                ));
                continue;
            }
            let parts: Vec<_> = call.name.iter().collect();
            if parts.len() < 3
                || parts.len() % 2 == 0
                || call.arguments.len() != parts.len() / 2 + 1
            {
                return Err(unsupported());
            }
            for (i, part) in parts.iter().enumerate() {
                if i % 2 == 0 {
                    if !matches!(part, NamePart::Slot) {
                        return Err(unsupported());
                    }
                } else {
                    let NamePart::Word { text, .. } = part else {
                        return Err(unsupported());
                    };
                    if !["<", "<=", ">", ">=", "=", "!="].contains(&text.as_str()) {
                        return Err(unsupported());
                    }
                    tests.push(Test::Compare(
                        expr(&call.arguments[i / 2])?,
                        text.clone(),
                        expr(&call.arguments[i / 2 + 1])?,
                    ));
                }
            }
        }
        if !tests.iter().any(Test::uses_value) {
            return Err(
                "found a predicate that never uses `_`; expected a predicate of the value".into(),
            );
        }
        let source = nodes.iter().map(node_text).collect::<Vec<_>>().join(", ");
        Ok(Self {
            source,
            tests,
            path: String::new(),
            span: Span {
                start: nodes[0].span.start,
                end: nodes.last().unwrap().span.end,
            },
        })
    }

    /// A description for an agent, with both bounds of a range spelled out.
    pub fn words(&self) -> String {
        self.tests
            .iter()
            .map(Test::words)
            .collect::<Vec<_>>()
            .join(" and ")
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
            .find(|test| !test.accepts(value))
            .map(Test::words)
    }

    /// The comparisons and text tests that must all hold.
    pub fn tests(&self) -> &[Test] {
        &self.tests
    }
}

fn unsupported() -> String {
    "unsupported predicate; expected a comparison chain such as `10 <= _ < 30`, a text test such as `_ ends with \".poentaro\"`, or a comparison of `(_ length)`; put arithmetic in parentheses, as in `(_ + 1) <= 30`".into()
}

fn expr(node: &Node) -> Result<Expr, String> {
    Ok(match &node.kind {
        NodeKind::Underscore => Expr::Value,
        // `(_)` deliberately retains its scope in the anyx tree.
        NodeKind::Parens(items) if items.len() == 1 => return expr(&items[0]),
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
        NodeKind::Call(call)
            if call.name_text() == "_ length"
                && call.arguments.len() == 1
                && matches!(call.arguments[0].kind, NodeKind::Underscore) =>
        {
            Expr::Length
        }
        NodeKind::Call(call) if call.arguments.len() == 2 => {
            let a = Box::new(expr(&call.arguments[0])?);
            let b = Box::new(expr(&call.arguments[1])?);
            match call.name_text().as_str() {
                "_ + _" => Expr::Add(a, b),
                "_ - _" => Expr::Sub(a, b),
                "_ * _" if a.constant() || b.constant() => Expr::Mul(a, b),
                _ => return Err(unsupported()),
            }
        }
        _ => return Err(unsupported()),
    })
}

impl Expr {
    fn any(&self, f: fn(&Self) -> bool) -> bool {
        f(self)
            || match self {
                Self::Add(a, b) | Self::Sub(a, b) | Self::Mul(a, b) => a.any(f) || b.any(f),
                _ => false,
            }
    }
    fn has_value(&self) -> bool {
        self.any(|e| matches!(e, Self::Value))
    }
    pub fn has_length(&self) -> bool {
        self.any(|e| matches!(e, Self::Length))
    }
    pub fn has_float(&self) -> bool {
        self.any(|e| matches!(e, Self::Float(_)))
    }
    fn constant(&self) -> bool {
        !self.has_value() && !self.has_length()
    }
    fn integer(&self, value: &Value) -> Option<i128> {
        match self {
            Self::Value => {
                if let Value::Integer(n) = value {
                    Some(*n as i128)
                } else {
                    None
                }
            }
            Self::Length => length(value).map(|n| n as i128),
            Self::Integer(n) => Some(*n as i128),
            Self::Float(_) => None,
            Self::Add(a, b) => a.integer(value)?.checked_add(b.integer(value)?),
            Self::Sub(a, b) => a.integer(value)?.checked_sub(b.integer(value)?),
            Self::Mul(a, b) => a.integer(value)?.checked_mul(b.integer(value)?),
        }
    }
    fn float(&self, value: &Value) -> Option<f64> {
        let n = match self {
            Self::Value => {
                if let Value::Float(n) = value {
                    *n
                } else {
                    return None;
                }
            }
            Self::Length => length(value)? as f64,
            Self::Integer(n) => *n as f64,
            Self::Float(n) => *n,
            Self::Add(a, b) => a.float(value)? + b.float(value)?,
            Self::Sub(a, b) => a.float(value)? - b.float(value)?,
            Self::Mul(a, b) => a.float(value)? * b.float(value)?,
        };
        n.is_finite().then_some(n)
    }
    fn text(&self) -> String {
        match self {
            Self::Value => "_".into(),
            Self::Length => "(_ length)".into(),
            Self::Integer(n) => n.to_string(),
            Self::Float(n) => format!("{n:?}"),
            Self::Add(a, b) => format!("({} + {})", a.text(), b.text()),
            Self::Sub(a, b) => format!("({} - {})", a.text(), b.text()),
            Self::Mul(a, b) => format!("({} * {})", a.text(), b.text()),
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
impl Test {
    fn uses_value(&self) -> bool {
        match self {
            Self::Text(..) => true,
            Self::Compare(a, _, b) => !a.constant() || !b.constant(),
        }
    }
    fn accepts(&self, value: &Value) -> bool {
        match self {
            Self::Text(op, text) => {
                if let Value::Text(s) = value {
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
                if matches!(value, Value::Float(_)) {
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
    pub fn words(&self) -> String {
        match self {
            Self::Text(op, text) => format!("{op} {text:?}"),
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
                    return format!("{} {op} {}", a.text(), b.text());
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
                    Expr::Value => String::new(),
                    Expr::Length => "length ".into(),
                    e => format!("{} ", e.text()),
                };
                format!("{subject}{relation} {}", bound.text())
            }
        }
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
                            } else if !field.optional {
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
                    | (Type::Boolean, Value::Boolean(_))
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
                        Test::Text(op, text) => match op.as_str() {
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
            Type::Boolean => vec![Value::Boolean(true)],
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
