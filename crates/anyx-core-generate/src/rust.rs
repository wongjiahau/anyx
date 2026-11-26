//! The Rust module of a schema: its types, a `Server` trait of its served
//! functions, its bodied functions compiled as written, and `handle_request`,
//! which answers a JSON request of entry-point calls.

use crate::Error;
use crate::model::{Argument, Function, Primitive, Schema, Ty, spine};
use crate::names::{camel_case, pascal_case, quoted, snake_case, type_name};
use anyx_core::kernel::{self, Term};
use anyx_core::lowering::{self, BindingIdentifier, GlobalKind};
use std::fmt::Write;

#[derive(Clone, Copy)]
enum Context {
    /// Inside a type declaration: an opaque type is a generic parameter.
    Definition,
    /// In the `Server` trait: an opaque type is an associated type.
    Trait,
    /// In generated functions, generic over `S: Server`.
    Compiled,
}

pub fn rust(schema: &Schema) -> Result<String, Error> {
    let mut out = String::new();
    out.push_str(PRELUDE);
    for data in &schema.datas {
        if crate::model::is_prelude(&data.name) || data.opaque {
            continue;
        }
        declare_type(schema, &mut out, data);
    }
    declare_server(schema, &mut out)?;
    for function in schema.served() {
        if function.split.is_some() {
            declare_dispatch(schema, &mut out, function)?;
        }
    }
    for function in schema.bodied() {
        declare_compiled(schema, &mut out, function)?;
    }
    declare_handle_request(schema, &mut out)?;
    Ok(out)
}

const PRELUDE: &str = r#"use serde::{Deserialize, Serialize};
use serde::de::DeserializeOwned;

/// A value whose type is only known at run time.
pub type Erased = serde_json::Value;

/// The type with no values.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Never {}

/// Move a value to a type that spells some of its parameters as `Erased`.
pub fn erase<T: Serialize, U: DeserializeOwned>(value: T) -> U {
    serde_json::from_value(serde_json::to_value(value).expect("generated types serialize"))
        .expect("an erased type reads what its value wrote")
}

/// Instants and durations travel as whole milliseconds.
pub mod wire {
    use serde::{Deserialize, Deserializer, Serializer};
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    fn millis(instant: &SystemTime) -> i64 {
        match instant.duration_since(UNIX_EPOCH) {
            Ok(after) => after.as_millis() as i64,
            Err(before) => -(before.duration().as_millis() as i64),
        }
    }

    fn from_millis(millis: i64) -> SystemTime {
        if millis >= 0 {
            UNIX_EPOCH + Duration::from_millis(millis as u64)
        } else {
            UNIX_EPOCH - Duration::from_millis(millis.unsigned_abs())
        }
    }

    pub mod instant {
        use super::*;
        pub fn serialize<S: Serializer>(instant: &SystemTime, serializer: S) -> Result<S::Ok, S::Error> {
            serializer.serialize_i64(millis(instant))
        }
        pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<SystemTime, D::Error> {
            Ok(from_millis(i64::deserialize(deserializer)?))
        }
    }

    pub mod optional_instant {
        use super::*;
        pub fn serialize<S: Serializer>(instant: &Option<SystemTime>, serializer: S) -> Result<S::Ok, S::Error> {
            match instant {
                Some(instant) => serializer.serialize_some(&millis(instant)),
                None => serializer.serialize_none(),
            }
        }
        pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<SystemTime>, D::Error> {
            Ok(Option::<i64>::deserialize(deserializer)?.map(from_millis))
        }
    }

    pub mod duration {
        use super::*;
        pub fn serialize<S: Serializer>(duration: &Duration, serializer: S) -> Result<S::Ok, S::Error> {
            serializer.serialize_u64(duration.as_millis() as u64)
        }
        pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Duration, D::Error> {
            Ok(Duration::from_millis(u64::deserialize(deserializer)?))
        }
    }
}

fn decode<T: DeserializeOwned>(arguments: &serde_json::Value, name: &str) -> Result<T, String> {
    let value = arguments
        .get(name)
        .cloned()
        .ok_or_else(|| format!("missing argument `{name}`"))?;
    serde_json::from_value(value).map_err(|error| format!("argument `{name}`: {error}"))
}

"#;

fn ty(schema: &Schema, ty: &Ty, context: Context) -> String {
    match ty {
        Ty::Primitive(primitive) => match primitive {
            Primitive::Text => "String".into(),
            Primitive::Integer => "i64".into(),
            Primitive::Instant => "::std::time::SystemTime".into(),
            Primitive::Duration => "::std::time::Duration".into(),
            Primitive::Boolean => "bool".into(),
            Primitive::Nothing => "()".into(),
            Primitive::Never => "Never".into(),
        },
        Ty::List(item) => format!("Vec<{}>", self::ty(schema, item, context)),
        Ty::Optional(item) => format!("Option<{}>", self::ty(schema, item, context)),
        Ty::Data { name, arguments } => {
            let data = schema.data(name);
            let mut generics: Vec<String> = arguments
                .iter()
                .map(|argument| self::ty(schema, argument, context))
                .collect();
            generics.extend(
                data.opaque_fields
                    .iter()
                    .map(|opaque| opaque_ty(opaque, context)),
            );
            if generics.is_empty() {
                type_name(name)
            } else {
                format!("{}<{}>", type_name(name), generics.join(", "))
            }
        }
        Ty::Variable(name) => name.clone(),
        Ty::Opaque(name) => opaque_ty(name, context),
        Ty::Erased => "Erased".into(),
    }
}

fn opaque_ty(name: &str, context: Context) -> String {
    match context {
        Context::Definition => pascal_case(name),
        Context::Trait => format!("Self::{}", pascal_case(name)),
        Context::Compiled => format!("S::{}", pascal_case(name)),
    }
}

/// The `serde(with)` module a field of this type needs, if any.
fn wire(ty: &Ty) -> Option<&'static str> {
    match ty {
        Ty::Primitive(Primitive::Instant) => Some("wire::instant"),
        Ty::Primitive(Primitive::Duration) => Some("wire::duration"),
        Ty::Optional(item) if **item == Ty::Primitive(Primitive::Instant) => {
            Some("wire::optional_instant")
        }
        _ => None,
    }
}

fn declare_type(schema: &Schema, out: &mut String, data: &crate::model::Data) {
    let mut generics = data.generics.clone();
    generics.extend(data.opaque_fields.iter().map(|opaque| pascal_case(opaque)));
    let generics = if generics.is_empty() {
        String::new()
    } else {
        format!("<{}>", generics.join(", "))
    };
    let serializable = data.opaque_fields.is_empty();
    let name = type_name(&data.name);
    let field = |out: &mut String, field: &str, field_ty: &Ty, public: bool| {
        if serializable {
            let _ = write!(out, "    #[serde(rename = {}", quoted(&camel_case(field)));
            if let Some(module) = wire(field_ty) {
                let _ = write!(out, ", with = {}", quoted(module));
            }
            out.push_str(")]\n");
        }
        let _ = writeln!(
            out,
            "    {}{}: {},",
            if public { "pub " } else { "" },
            snake_case(field),
            ty(schema, field_ty, Context::Definition)
        );
    };
    let _ = writeln!(out, "/// `{}`.", data.name);
    if serializable {
        out.push_str("#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]\n");
    } else {
        out.push_str("#[derive(Clone, Debug, PartialEq)]\n");
    }
    if let [constructor] = data.constructors.as_slice() {
        if serializable {
            let _ = writeln!(
                out,
                "#[serde(tag = \"kind\", rename = {})]",
                quoted(&constructor.kind())
            );
        }
        let _ = writeln!(out, "pub struct {name}{generics} {{");
        for (field_name, field_ty) in constructor.fields() {
            field(out, field_name, field_ty, true);
        }
        out.push_str("}\n\n");
        return;
    }
    if serializable {
        out.push_str("#[serde(tag = \"kind\")]\n");
    }
    let _ = writeln!(out, "pub enum {name}{generics} {{");
    for constructor in &data.constructors {
        if serializable {
            let _ = writeln!(
                out,
                "    #[serde(rename = {})]",
                quoted(&constructor.kind())
            );
        }
        let fields: Vec<_> = constructor.fields().collect();
        if fields.is_empty() {
            let _ = writeln!(out, "    {},", constructor.variant());
            continue;
        }
        let _ = writeln!(out, "    {} {{", constructor.variant());
        for (field_name, field_ty) in fields {
            let mut inner = String::new();
            field(&mut inner, field_name, field_ty, false);
            for line in inner.lines() {
                let _ = writeln!(out, "    {line}");
            }
        }
        out.push_str("    },\n");
    }
    out.push_str("}\n\n");
}

/// One method per served function, or per constructor of a split one.
struct Method {
    name: String,
    parameters: Vec<(String, Ty)>,
    result: Ty,
    doc: String,
}

fn methods(schema: &Schema, function: &Function) -> Result<Vec<Method>, Error> {
    let Some(split) = &function.split else {
        let mut parameters = Vec::new();
        for (position, parameter) in function.parameters.iter().enumerate() {
            if !parameter.erased {
                parameters.push((
                    parameter.name.clone(),
                    schema.erased_ty(function, Some(position))?,
                ));
            }
        }
        return Ok(vec![Method {
            name: snake_case(&function.name),
            parameters,
            result: schema.erased_ty(function, None)?,
            doc: format!("`{}`.", function.name),
        }]);
    };
    let only_split = schema
        .served()
        .filter(|served| served.split.is_some())
        .count()
        == 1;
    let mut methods = Vec::new();
    for case in &split.cases {
        let mut parameters = Vec::new();
        for (position, argument) in case.parameters.iter().enumerate() {
            match argument {
                Argument::Split => {
                    for (name, field_ty) in schema.constructor(&case.constructor).fields() {
                        parameters.push((name.to_string(), field_ty.clone()));
                    }
                }
                Argument::Collapsed { fields, .. } => parameters.extend(fields.iter().cloned()),
                Argument::Plain { ty, .. } => {
                    parameters.push((function.parameters[position].name.clone(), ty.clone()));
                }
                Argument::Erased | Argument::Solved(_) => {}
            }
        }
        methods.push(Method {
            name: if only_split {
                snake_case(&case.constructor)
            } else {
                format!(
                    "{}_{}",
                    snake_case(&function.name),
                    snake_case(&case.constructor)
                )
            },
            parameters,
            result: case.result.clone(),
            doc: format!("`{}` at `{}`.", function.name, case.constructor),
        });
    }
    Ok(methods)
}

fn declare_server(schema: &Schema, out: &mut String) -> Result<(), Error> {
    out.push_str(
        "/// The served functions: a `FUNCTION` without a body is implemented here.\n\
         #[allow(async_fn_in_trait)]\npub trait Server {\n",
    );
    for data in &schema.datas {
        if data.opaque {
            let _ = writeln!(
                out,
                "    /// `{}`: its values never leave the server, which chooses how to hold them.\n    type {};",
                data.name,
                type_name(&data.name)
            );
        }
    }
    for function in schema.served() {
        for method in methods(schema, function)? {
            let parameters: Vec<String> = unique(&method.parameters)
                .iter()
                .map(|(name, parameter_ty)| {
                    format!("{name}: {}", ty(schema, parameter_ty, Context::Trait))
                })
                .collect();
            let _ = writeln!(
                out,
                "    /// {}\n    async fn {}(&self{}{}) -> {};",
                method.doc,
                method.name,
                if parameters.is_empty() { "" } else { ", " },
                parameters.join(", "),
                ty(schema, &method.result, Context::Trait)
            );
        }
    }
    out.push_str("}\n\n");
    Ok(())
}

/// Spell names as distinct identifiers.
fn unique(parameters: &[(String, Ty)]) -> Vec<(String, Ty)> {
    let mut seen: Vec<String> = Vec::new();
    parameters
        .iter()
        .map(|(name, parameter_ty)| {
            let mut identifier = snake_case(name);
            while seen.contains(&identifier) {
                identifier.push('_');
            }
            seen.push(identifier.clone());
            (identifier, parameter_ty.clone())
        })
        .collect()
}

/// A pattern binding a constructor's fields to `bindings`, or ignoring them.
fn constructor_pattern(schema: &Schema, constructor: &str, bindings: Option<&[String]>) -> String {
    let constructor = schema.constructor(constructor);
    let data = schema.data(&constructor.data);
    let path = if data.constructors.len() == 1 {
        type_name(&data.name)
    } else {
        format!("{}::{}", type_name(&data.name), constructor.variant())
    };
    let fields: Vec<_> = constructor.fields().collect();
    match bindings {
        None if fields.is_empty() && data.constructors.len() > 1 => path,
        None => format!("{path} {{ .. }}"),
        Some(bindings) => {
            if fields.is_empty() && data.constructors.len() > 1 {
                return path;
            }
            let parts: Vec<String> = fields
                .iter()
                .zip(bindings)
                .map(|((field, _), binding)| format!("{}: {binding}", snake_case(field)))
                .collect();
            format!("{path} {{ {} }}", parts.join(", "))
        }
    }
}

fn dispatch_name(function: &Function) -> String {
    format!("call_{}", snake_case(&function.name))
}

/// The parameters a generated function takes: those not of `Type`.
fn compiled_parameters(schema: &Schema, function: &Function) -> Result<Vec<String>, Error> {
    let mut parameters = Vec::new();
    for (position, parameter) in function.parameters.iter().enumerate() {
        if !parameter.erased {
            parameters.push(format!(
                "{}: {}",
                local_name(position, &parameter.name),
                ty(
                    schema,
                    &schema.erased_ty(function, Some(position))?,
                    Context::Compiled
                )
            ));
        }
    }
    Ok(parameters)
}

fn local_name(position: usize, name: &str) -> String {
    format!("{}_{position}", snake_case(name).trim_start_matches("r#"))
}

fn declare_dispatch(schema: &Schema, out: &mut String, function: &Function) -> Result<(), Error> {
    let split = function
        .split
        .as_ref()
        .expect("dispatch is for split functions");
    let methods = methods(schema, function)?;
    let _ = writeln!(
        out,
        "/// Calls `{}` by its implementation for the constructor of `{}`.\n\
         #[allow(unused_variables, unreachable_patterns, irrefutable_let_patterns)]\n\
         pub async fn {}<S: Server>(server: &S, {}) -> {} {{",
        function.name,
        function.parameters[split.position].name,
        dispatch_name(function),
        compiled_parameters(schema, function)?.join(", "),
        ty(
            schema,
            &schema.erased_ty(function, None)?,
            Context::Compiled
        )
    );
    let _ = writeln!(
        out,
        "    match {} {{",
        local_name(split.position, &function.parameters[split.position].name)
    );
    for (case, method) in split.cases.iter().zip(&methods) {
        let mut arguments = Vec::new();
        let mut fresh = 0;
        let mut next = || {
            fresh += 1;
            format!("field_{fresh}")
        };
        let split_bindings: Vec<String> = schema
            .constructor(&case.constructor)
            .fields()
            .map(|_| next())
            .collect();
        let _ = writeln!(
            out,
            "        {} => {{",
            constructor_pattern(schema, &case.constructor, Some(&split_bindings))
        );
        for (position, argument) in case.parameters.iter().enumerate() {
            match argument {
                Argument::Split => arguments.extend(split_bindings.iter().cloned()),
                Argument::Collapsed {
                    constructor,
                    fields,
                } => {
                    let bindings: Vec<String> = fields.iter().map(|_| next()).collect();
                    let _ = writeln!(
                        out,
                        "            let {} = {} else {{ unreachable!(\"the checker proved `{}` fits `{}`\") }};",
                        constructor_pattern(schema, constructor, Some(&bindings)),
                        local_name(position, &function.parameters[position].name),
                        function.parameters[position].name,
                        constructor
                    );
                    arguments.extend(bindings);
                }
                Argument::Plain { .. } => {
                    arguments.push(local_name(position, &function.parameters[position].name));
                }
                Argument::Erased | Argument::Solved(_) => {}
            }
        }
        let _ = writeln!(
            out,
            "            erase(server.{}({}).await)\n        }}",
            method.name,
            arguments.join(", ")
        );
    }
    out.push_str("    }\n}\n\n");
    Ok(())
}

fn declare_compiled(schema: &Schema, out: &mut String, function: &Function) -> Result<(), Error> {
    let body = function
        .body
        .as_ref()
        .expect("compiled functions have bodies");
    let mut compiler = Compiler {
        schema,
        locals: Vec::new(),
        next: 0,
    };
    for (position, (binding, parameter)) in function
        .bindings
        .iter()
        .zip(&function.parameters)
        .enumerate()
    {
        compiler
            .locals
            .push((*binding, local_name(position, &parameter.name)));
    }
    let body = compiler
        .term(body)
        .map_err(|error| Error::new(format!("`{}`: {error}", function.name)))?;
    let _ = writeln!(
        out,
        "/// `{}`, as written in the schema.\n\
         #[allow(unused_variables, unreachable_patterns, unreachable_code)]\n\
         pub async fn {}<S: Server>(server: &S, {}) -> {} {{\n    {body}\n}}\n",
        function.name,
        snake_case(&function.name),
        compiled_parameters(schema, function)?.join(", "),
        ty(
            schema,
            &schema.erased_ty(function, None)?,
            Context::Compiled
        )
    );
    Ok(())
}

struct Compiler<'schema> {
    schema: &'schema Schema,
    locals: Vec<(BindingIdentifier, String)>,
    next: usize,
}

impl Compiler<'_> {
    fn bind(&mut self, binding: &lowering::Binding) -> String {
        self.next += 1;
        let name = format!(
            "{}_{}",
            snake_case(binding.name.as_deref().unwrap_or("value")).trim_start_matches("r#"),
            self.next
        );
        self.locals.push((binding.identifier, name.clone()));
        name
    }

    fn term(&mut self, term: &lowering::Term) -> Result<String, String> {
        match term {
            lowering::Term::Local(identifier) => self
                .locals
                .iter()
                .rev()
                .find(|(bound, _)| bound == identifier)
                .map(|(_, name)| name.clone())
                .ok_or_else(|| "a local is out of scope".to_string()),
            lowering::Term::Global { name, kind } => self.call(name, *kind, &[]),
            lowering::Term::Apply {
                function,
                arguments,
            } => {
                let lowering::Term::Global { name, kind } = function.as_ref() else {
                    return Err("only calls of named functions are compiled".into());
                };
                self.call(name, *kind, arguments)
            }
            lowering::Term::Match { subject, cases } => {
                let subject = self.term(subject)?;
                let mut arms = Vec::new();
                for case in cases {
                    let scope = self.locals.len();
                    let pattern = self.pattern(&case.pattern, false)?;
                    let result = self.term(&case.result)?;
                    self.locals.truncate(scope);
                    arms.push(format!("{pattern} => {{ {result} }}"));
                }
                arms.push("_ => unreachable!(\"the checker ruled this case out\")".into());
                Ok(format!(
                    "match {subject} {{\n        {}\n    }}",
                    arms.join(",\n        ")
                ))
            }
            _ => Err("only names, calls and MATCH are compiled".into()),
        }
    }

    fn call(
        &mut self,
        name: &str,
        kind: GlobalKind,
        arguments: &[lowering::Term],
    ) -> Result<String, String> {
        match kind {
            GlobalKind::Constructor => {
                let constructor = self.schema.constructor(name);
                let mut values = Vec::new();
                for (position, argument) in arguments.iter().enumerate() {
                    if !constructor.erased(position) {
                        values.push(self.term(argument)?);
                    }
                }
                Ok(construct(self.schema, name, &values))
            }
            GlobalKind::Function => {
                let function = self
                    .schema
                    .function(name)
                    .ok_or_else(|| format!("`{name}` computes a type, not a value"))?;
                let mut values = Vec::new();
                for (argument, parameter) in arguments.iter().zip(&function.parameters) {
                    if !parameter.erased {
                        values.push(self.term(argument)?);
                    }
                }
                let values = values.join(", ");
                let separator = if values.is_empty() { "" } else { ", " };
                Ok(match (&function.body, &function.split) {
                    (None, Some(_)) => {
                        format!(
                            "{}(server{separator}{values}).await",
                            dispatch_name(function)
                        )
                    }
                    (None, None) => format!("server.{}({values}).await", snake_case(name)),
                    (Some(_), _) => {
                        format!("{}(server{separator}{values}).await", snake_case(name))
                    }
                })
            }
            GlobalKind::Data => Err(format!("the type `{name}` is not a value")),
        }
    }

    fn pattern(&mut self, pattern: &lowering::Pattern, erased: bool) -> Result<String, String> {
        match pattern {
            lowering::Pattern::Binding(binding) => {
                let name = self.bind(binding);
                Ok(if erased { "_".into() } else { name })
            }
            lowering::Pattern::Constructor { name, arguments } => {
                let constructor = self.schema.constructor(name);
                let mut fields = Vec::new();
                for (position, argument) in arguments.iter().enumerate() {
                    let field = self.pattern(argument, constructor.erased(position))?;
                    if !constructor.erased(position) {
                        fields.push(field);
                    }
                }
                Ok(match prelude_constructor(name) {
                    Some(spelling) => {
                        spelling.replace("{}", fields.first().map_or("", String::as_str))
                    }
                    None => constructor_pattern(self.schema, name, Some(&fields)),
                })
            }
        }
    }
}

/// How the prelude's constructors are spelled in Rust.
fn prelude_constructor(name: &str) -> Option<&'static str> {
    Some(match name {
        "nothing of _" => "None",
        "something of _ is _" => "Some({})",
        "true" => "true",
        "false" => "false",
        "done" => "()",
        _ => return None,
    })
}

fn construct(schema: &Schema, name: &str, values: &[String]) -> String {
    if let Some(spelling) = prelude_constructor(name) {
        return spelling.replace("{}", values.first().map_or("", String::as_str));
    }
    let constructor = schema.constructor(name);
    let data = schema.data(&constructor.data);
    let fields: Vec<String> = constructor
        .fields()
        .zip(values)
        .map(|((field, _), value)| format!("{}: {value}", snake_case(field)))
        .collect();
    if data.constructors.len() == 1 {
        format!("{} {{ {} }}", type_name(&data.name), fields.join(", "))
    } else if fields.is_empty() {
        format!("{}::{}", type_name(&data.name), constructor.variant())
    } else {
        format!(
            "{}::{} {{ {} }}",
            type_name(&data.name),
            constructor.variant(),
            fields.join(", ")
        )
    }
}

/// A closed constructor tree, such as a solved index.
fn closed_value(schema: &Schema, term: &Term) -> Result<String, Error> {
    let (head, arguments) = spine(term);
    let Term::Global(kernel::Global {
        name,
        kind: GlobalKind::Constructor,
    }) = head
    else {
        return Err(Error::new("a solved value must be a constructor tree"));
    };
    let constructor = schema.constructor(name);
    let mut values = Vec::new();
    for (position, argument) in arguments.iter().enumerate() {
        if !constructor.erased(position) {
            values.push(closed_value(schema, argument)?);
        }
    }
    Ok(construct(schema, name, &values))
}

fn declare_handle_request(schema: &Schema, out: &mut String) -> Result<(), Error> {
    let mut arms = String::new();
    let mut names = String::new();
    for function in schema.entries() {
        let split = function.split.as_ref().expect("entries are split");
        let split_name = &function.parameters[split.position].name;
        let split_local = local_name(split.position, split_name);
        let _ = writeln!(
            names,
            "        {} => call.get(\"arguments\").and_then(|arguments| arguments.get({})).and_then(|value| value.get(\"kind\")).and_then(|kind| kind.as_str()).map(str::to_string),",
            quoted(&function.name),
            quoted(&camel_case(split_name))
        );
        let _ = writeln!(
            arms,
            "        {} => {{\n            let {split_local}: {} = decode(&arguments, {})?;\n            match &{split_local} {{",
            quoted(&function.name),
            ty(
                schema,
                &schema.erased_ty(function, Some(split.position))?,
                Context::Compiled
            ),
            quoted(&camel_case(split_name))
        );
        for case in &split.cases {
            let _ = writeln!(
                arms,
                "                {} => {{",
                constructor_pattern(schema, &case.constructor, None)
            );
            let mut values = Vec::new();
            for (position, argument) in case.parameters.iter().enumerate() {
                let parameter = &function.parameters[position];
                let local = local_name(position, &parameter.name);
                match argument {
                    Argument::Erased => continue,
                    Argument::Split => {
                        values.push(format!("{split_local}.clone()"));
                        continue;
                    }
                    Argument::Solved(value) => {
                        let _ = writeln!(
                            arms,
                            "                    let {local} = {};",
                            closed_value(schema, value)?
                        );
                    }
                    Argument::Collapsed { constructor, .. } => {
                        let _ = writeln!(
                            arms,
                            "                    let {local}: {} = decode(&arguments, {})?;\n                    if !matches!({local}, {}) {{ return Err({}.to_string()); }}",
                            ty(
                                schema,
                                &schema.erased_ty(function, Some(position))?,
                                Context::Compiled
                            ),
                            quoted(&camel_case(&parameter.name)),
                            constructor_pattern(schema, constructor, None),
                            quoted(&format!(
                                "`{}` doesn't fit `{}`",
                                parameter.name, case.constructor
                            ))
                        );
                    }
                    Argument::Plain { fitting, .. } => {
                        let _ = writeln!(
                            arms,
                            "                    let {local}: {} = decode(&arguments, {})?;",
                            ty(
                                schema,
                                &schema.erased_ty(function, Some(position))?,
                                Context::Compiled
                            ),
                            quoted(&camel_case(&parameter.name))
                        );
                        if let Some(fitting) = fitting {
                            let patterns: Vec<String> = fitting
                                .iter()
                                .map(|constructor| constructor_pattern(schema, constructor, None))
                                .collect();
                            let patterns = if patterns.is_empty() {
                                "_ if false".to_string()
                            } else {
                                patterns.join(" | ")
                            };
                            let _ = writeln!(
                                arms,
                                "                    if !matches!({local}, {patterns}) {{ return Err({}.to_string()); }}",
                                quoted(&format!(
                                    "`{}` doesn't fit `{}`",
                                    parameter.name, case.constructor
                                ))
                            );
                        }
                    }
                }
                values.push(local);
            }
            let _ = writeln!(
                arms,
                "                    Ok(serde_json::to_value({}(server, {}).await).expect(\"generated types serialize\"))\n                }}",
                snake_case(&function.name),
                values.join(", ")
            );
        }
        arms.push_str("            }\n        }\n");
    }
    let _ = write!(
        out,
        r#"/// Answers a request: a JSON list of calls, each
/// `{{"function": <entry point>, "arguments": {{<parameter>: <value>}}}}`.
/// The answer lists each call's result, or
/// `{{"kind": "invalid request", "message": ...}}` for a call that doesn't
/// fit its entry point's types.
pub async fn handle_request<S: Server>(server: &S, body: &str) -> String {{
    let answers = match serde_json::from_str::<Vec<serde_json::Value>>(body) {{
        Ok(calls) => {{
            let mut answers = Vec::new();
            for call in &calls {{
                answers.push(match answer(server, call).await {{
                    Ok(value) => value,
                    Err(message) => serde_json::json!({{"kind": "invalid request", "message": message}}),
                }});
            }}
            answers
        }}
        Err(error) => vec![serde_json::json!({{"kind": "invalid request", "message": error.to_string()}})],
    }};
    serde_json::to_string(&answers).expect("JSON values serialize")
}}

/// The constructor each call of a request names, for logs.
pub fn called(body: &str) -> Vec<String> {{
    let calls: Vec<serde_json::Value> = serde_json::from_str(body).unwrap_or_default();
    calls
        .iter()
        .filter_map(|call| match call.get("function").and_then(|function| function.as_str())? {{
{names}            _ => None,
        }})
        .collect()
}}

#[allow(unused_variables, unreachable_patterns)]
async fn answer<S: Server>(server: &S, call: &serde_json::Value) -> Result<serde_json::Value, String> {{
    let function = call
        .get("function")
        .and_then(|function| function.as_str())
        .ok_or("a call names its `function`")?;
    let arguments = call.get("arguments").cloned().unwrap_or(serde_json::Value::Null);
    match function {{
{arms}        _ => Err(format!("no entry point is called `{{function}}`")),
    }}
}}
"#
    );
    Ok(())
}
