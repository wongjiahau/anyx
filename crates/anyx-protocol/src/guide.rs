//! The guide a server sends to a client that has never seen the protocol,
//! such as an AI agent: how to write a request and read a response, then the
//! schema as written.

use crate::schema::{Method, Schema, Type, TypeKind, scoped};

/// The guide to `schema`. `how_to_send` tells the reader where and how to
/// send a request, such as the URL and the header to send it with; when it
/// is empty, as in the answer to `SCHEMA`, the guide leaves it out.
pub fn guide(schema: &Schema, how_to_send: &str) -> String {
    // The schema's own examples show a call best; without any, a call is
    // made up with a sample value in each slot.
    let has_examples = |method: &&Method| !method.examples.is_empty();
    let shown: Vec<&Method> = match schema.methods.iter().any(|method| has_examples(&method)) {
        true => schema.methods.iter().filter(has_examples).collect(),
        false => schema.methods.iter().collect(),
    };
    let widest = shown
        .iter()
        .copied()
        .max_by_key(|method| method.shape.parameters.len());
    let example = widest.map_or(String::new(), |method| call_of(schema, method));
    let scoped_example = match widest.filter(|method| !method.scope.is_empty()) {
        Some(_) => "\nA method in a scope is called by its full name, as above: the scope's\npath, `/`, then the call.\n".to_string(),
        None => shown
            .iter()
            .copied()
            .find(|method| !method.scope.is_empty())
            .or_else(|| schema.methods.iter().find(|method| !method.scope.is_empty()))
            .map_or(String::new(), |method| {
                format!(
                    "\nA method in a scope is called by its full name: the scope's path, `/`,\nthen the call:\n\n    {}\n",
                    call_of(schema, method)
                )
            }),
    };
    format!(
        "# {name} (anyx protocol, version {version})

{how_to_send}## Requests

A request is a list of calls separated by commas. Each call is written in the
shape of one of the methods below, with a value in each `(name: TYPE)` slot:

    {example}
{scoped_example}
Every call runs, in order, and the response has one result per call.

`SCHEMA` is a call every server answers, with this guide.

## Values

- TEXT is a string: `\"some text\"`. Escape `\\\"` and `\\\\`, and write a line
  break as `\\n`. For text with many lines, use a raw string: three quotes,
  the lines, then three quotes on a line of their own. The closing quotes'
  indentation is removed from every line. A raw string has no escapes.

      \"\"\"
          first line
          second line
          \"\"\"

- BYTES is base64 in a string: `\"TVRoZA==\"`.
- INTEGER is a whole number, NUMBER any number: `3`, `-1.5`.
- BOOLEAN is `true` or `false`.
- INSTANT is a point in time, in UTC to the millisecond, as a string:
  `\"2026-11-14T06:00:08.000Z\"`.
- DURATION is whole numbers each followed by its unit, the largest first:
  `90 seconds`, `1 hour 30 minutes`. The units are `days`, `hours`,
  `minutes`, `seconds` and `milliseconds`, or `day`, `hour` and so on for 1.
- `LIST: T` is `[a, b, c]`.
- `MAP: T` is keys, each a string, with their values in braces:
  `{{ \"hymn\" 3, \"ode\" 1 }}`. Each key appears once. When a value is
  words, put `:` after its key: `{{ \"hymn\": done }}`.
- A RECORD is its fields in braces, each its name and value, in any order:
  `{{ path \"a\", revision 3 }}`. An `OPTIONAL` field may be left out; one
  with a `DEFAULT` then holds that value. When a value is words, put `:`
  after the field's name: `{{ status: done }}`, `{{ timeout: 5 minutes }}`.
- A CHOICE is one of its variants, written in the variant's shape with a
  value in each slot: `dark`, `custom hue 0.5`.
{generic_types}{named_types}- In a slot, a value written as words goes in parentheses so that it doesn't
  join the words around it: `set (true)`, `pick (custom hue 0.5)`,
  `wait (5 minutes)`.

## Responses

The response has one result per call, in order. Each result repeats its
call, with text over 32 characters and lists over 3 items cut short with
`…`, then one of:

- `RETURNS` and the value the method returns, or `RETURNS NOTHING` for a
  method that returns nothing.
- `ERROR` and one of the method's errors, in the error's shape. They follow
  `ERRORS`: its own in brackets, then those of each group joined with `+`.
  For example: `read \"a\" ERROR missing`. The call didn't do anything.
  Each error's `#` lines say what to do next.
- `INTERNAL ERROR`: the server failed. Calls after it don't run.
- `NOT RUN`: the call didn't run, because an earlier call failed that way.

A request that can't be read gets one result, and none of its calls run:

    INVALID REQUEST {{ line 1, column 6, message \"…\" }}

## The schema

In the schema, a `#` line before a definition says what it means. Lowercase
words are names; all-caps words belong to the schema language. `ERRORS name
[…]` is a group of errors that methods share by joining it with `+`.{examples}{scopes}

{source}
",
        name = schema.name,
        version = schema.version,
        how_to_send = match how_to_send.is_empty() {
            true => String::new(),
            false => format!("{how_to_send}\n\n"),
        },
        scopes = match schema.files.len() {
            1 => "",
            _ => SCOPES,
        },
        source = sources(schema),
        named_types = named_types(schema),
        generic_types = generic_types(schema),
        examples = match schema.methods.iter().any(|method| has_examples(&method)) {
            true => EXAMPLES,
            false => "",
        },
    )
}

/// The rule for generic types, shown with the schema's first one: "`GENERIC
/// (item) RECORD page`", then a use of it. Empty for a schema without any.
fn generic_types(schema: &Schema) -> String {
    let Some(definition) = schema
        .types
        .iter()
        .find(|definition| !definition.parameters.is_empty())
    else {
        return String::new();
    };
    let keyword = match definition.kind {
        TypeKind::Choice(_) => "CHOICE",
        TypeKind::Record(_) | TypeKind::Distinct(_) => "RECORD",
    };
    let name = scoped(&definition.scope, &definition.name);
    let arguments: Vec<String> = definition
        .parameters
        .iter()
        .map(|parameter| format!("{parameter}: TEXT"))
        .collect();
    format!(
        "- A GENERIC RECORD or CHOICE has type parameters, which its fields or\n  variants use as types: `GENERIC ({}) {keyword} {}`. A use gives a type\n  for each, `{name} WITH ({})`, and its value is written with those types\n  in place of the parameters.\n",
        definition.parameters.join(", "),
        definition.name,
        arguments.join(", "),
    )
}

/// The rule for named types, and each of them with the type it is written
/// as: "`file id`: `TEXT`", then its doc. Empty for a schema without any.
fn named_types(schema: &Schema) -> String {
    let named: Vec<String> = schema
        .types
        .iter()
        .filter_map(|definition| {
            let TypeKind::Distinct(underlying) = &definition.kind else {
                return None;
            };
            let doc = definition.doc.as_deref().map_or(String::new(), |doc| {
                format!(" {}", doc.split_whitespace().collect::<Vec<_>>().join(" "))
            });
            Some(format!(
                "  - `{}`: `{}`.{doc}\n",
                scoped(&definition.scope, &definition.name),
                schema.type_name(underlying)
            ))
        })
        .collect();
    match named.is_empty() {
        true => String::new(),
        false => format!(
            "- A named TYPE is written exactly like the type it names, but holds\n  only what its name says:\n{}",
            named.concat()
        ),
    }
}

const SCOPES: &str = "

The schema is a file per scope, each shown below under its full name. A
type or an error group is private to its scope unless it is `PUBLIC`; another
scope writes its full path, such as `common/diagnostic`. A response writes
an error in its shape alone, without its scope: `ERROR missing`.";

const EXAMPLES: &str = "
`EXAMPLES […]` after a method lists calls of it, each followed by its
result as a response writes it, with a `#` line saying what it shows.";

/// The schema's files as written: a schema of one file as it is, else each
/// file under a heading of its scope.
fn sources(schema: &Schema) -> String {
    if let [file] = schema.files.as_slice() {
        return file.source.trim_end().to_string();
    }
    schema
        .files
        .iter()
        .map(|file| {
            let methods: Vec<String> = schema
                .methods
                .iter()
                .filter(|method| method.scope == file.scope)
                .map(|method| format!("`{}`", method.full_name()))
                .collect();
            let heading = match file.scope.is_empty() {
                true => format!("### `{}`", file.path),
                false => format!("### `{}` (`{}`)", file.scope.join("/"), file.path),
            };
            let calls = match methods.is_empty() {
                true => String::new(),
                false => format!("Methods: {}.\n\n", methods.join(", ")),
            };
            format!("{heading}\n\n{calls}{}", file.source.trim_end())
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// A call of `method` by its full name: its first example's, or else one
/// with a sample value in each slot.
fn call_of(schema: &Schema, method: &Method) -> String {
    match method.examples.first() {
        Some(example) => example.call.clone(),
        None => sample_call(schema, method),
    }
}

/// A call of `method` by its full name, with a sample value in each slot.
fn sample_call(schema: &Schema, method: &Method) -> String {
    let mut values = method
        .shape
        .parameters
        .iter()
        .map(|parameter| sample(schema, &parameter.r#type));
    let call = method
        .shape
        .text
        .split(' ')
        .map(|word| match word {
            "_" => values.next().unwrap_or_default(),
            word => word.to_string(),
        })
        .collect::<Vec<_>>()
        .join(" ");
    scoped(&method.scope, &call)
}

/// A value of `type` as an example fills a slot with it.
fn sample(schema: &Schema, r#type: &Type) -> String {
    match schema.underlying(r#type) {
        Type::Text => "\"a\"".to_string(),
        Type::Bytes => "\"AQI=\"".to_string(),
        Type::Integer => "3".to_string(),
        Type::Number => "1.5".to_string(),
        Type::Boolean => "(true)".to_string(),
        Type::Instant => "\"2026-11-14T06:00:08.000Z\"".to_string(),
        Type::Duration => "(5 minutes)".to_string(),
        Type::List(_) => "[…]".to_string(),
        Type::Map(_) => "{ … }".to_string(),
        Type::Named(index) | Type::Generic(index, _) => match schema.types[*index].kind {
            TypeKind::Record(_) => "{ … }".to_string(),
            TypeKind::Choice(_) => "(…)".to_string(),
            TypeKind::Distinct(_) => unreachable!("`underlying` reads past named types"),
        },
        Type::Parameter(_) => "…".to_string(),
    }
}
