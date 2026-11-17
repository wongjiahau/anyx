//! The guide a server sends to a client that has never seen the protocol,
//! such as an AI agent: how to write a request and read a response, then the
//! schema as written.

use crate::schema::{Method, Schema, Type, TypeKind, fill_slots};

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
    format!(
        "# {name} (anyx protocol, version {version})

{how_to_send}## Requests

A request is a list of calls separated by commas. Each call is written in the
shape of one of the methods below, with a value in each `(name: TYPE)` slot:

    {example}
{overloads}
Every call runs, in order, and the response has one result per call.

`SCHEMA` is a call every server answers, with this guide.

## Effects

Words before `METHOD` say what a call of it may do, and decide when it is
safe to send without asking:

- `READ ONLY METHOD`: it changes nothing, so call it freely. `SCHEMA` is
  read only.
- `IDEMPOTENT METHOD`: calling it again with the same arguments changes
  nothing more, so it is safe to retry when a response was lost. A retry may
  still answer differently, such as with an error saying it is already done,
  because the first call went through.
- `METHOD` alone: it may change anything, and calling it twice may do it
  twice. Don't retry it blindly: check first whether the call went through,
  and ask the person you work for when unsure.

`UNDONE BY` after a method's results is the call that undoes it, with the
method's own arguments in its slots: `UNDONE BY move (new path: Text) to
(path: Text)` means that after `move \"a\" to \"b\"`, the call `move \"b\" to
\"a\"` takes it back. The state afterwards is equivalent, not always exactly
the same. A method without `UNDONE BY` can't be undone by a call: its
effect may be permanent, or it may have shown something to people outside
the account, such as by sending or sharing it.

## Values

{built_in}- `LIST: T` is `[a, b, c]`.
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

An argument whose type its value doesn't show is annotated with `AS:` and
its parameter's type, in parentheses: `tag ([] AS: LIST Text)`,
`open (\"a\" AS: File Id)`. A value shows its type when it is a number, a
string (`Text`), `true` or `false`, a duration, a list or map whose items
show one type, or a record or variant that only one type of the schema
fits. `[]`, `{{}}`, and a value of a named TYPE, a generic type, `Bytes`
or `Instant` never do. Without its annotation, the request is invalid.
Only a whole argument is annotated; a value inside it is read by the type
around it. An annotation states the type; it doesn't convert the value.

{refinements}## Responses

The response has one result per call, in the order of the calls, separated
by commas. A result doesn't repeat its call: the first result answers the
first call, the second the second, and so on, so keep a request short
enough to count its results without slipping. A result is one of:

- `RETURNS` and the value the method returns, or `RETURNS NOTHING` for a
  method that returns nothing.
- `ERROR` and one of the method's errors, in the error's shape. They follow
  `ERRORS`: its own in brackets, then those of each group joined with `+`.
  For example: `ERROR missing`. The call didn't do anything. Each error's
  `#` lines say what to do next.
- `INTERNAL ERROR`: the server failed. Calls after it don't run.
- `NOT RUN`: the call didn't run, because an earlier call failed that way.

For two calls, one that read a file and one whose file was missing:

    RETURNS {{ path \"a\", revision 3 }},
    ERROR missing,

A request that can't be read gets one result, and none of its calls run:

    INVALID REQUEST {{ line 1, column 6, message \"…\" }}

## The schema

In the schema, a `#` line before a definition says what it means. Types
are named in Title Case, such as `File Summary`; other names are lowercase
words; all-caps words belong to the schema language. `ERRORS name […]`
is a group of errors that methods share by joining it with `+`.{examples}{imports}

{source}
",
        name = schema.name,
        version = schema.version,
        how_to_send = match how_to_send.is_empty() {
            true => String::new(),
            false => format!("{how_to_send}\n\n"),
        },
        imports = match schema.files.len() {
            1 => "",
            _ => IMPORTS,
        },
        source = sources(schema),
        overloads = overloads(schema),
        named_types = named_types(schema),
        built_in = built_in_types(),
        generic_types = generic_types(schema),
        refinements = refinements(schema),
        examples = match schema.methods.iter().any(|method| has_examples(&method)) {
            true => EXAMPLES,
            false => "",
        },
    )
}

/// The methods that share a shape with another, each shape's together,
/// and how a call chooses one. Empty for a schema without any.
fn overloads(schema: &Schema) -> String {
    let mut shapes: Vec<Vec<&Method>> = Vec::new();
    for method in schema
        .methods
        .iter()
        .filter(|method| schema.is_overloaded(method))
    {
        match shapes
            .iter_mut()
            .find(|shape| shape[0].full_name() == method.full_name())
        {
            Some(shape) => shape.push(method),
            None => shapes.push(vec![method]),
        }
    }
    if shapes.is_empty() {
        return String::new();
    }
    let lines: String = shapes
        .iter()
        .map(|shape| {
            let typed: Vec<String> = shape
                .iter()
                .map(|method| format!("`{}`", schema.typed_shape(&method.shape)))
                .collect();
            format!("- {}\n", typed.join(", "))
        })
        .collect();
    format!(
        "\nSome methods share a shape, and differ in their parameters' types. A call\nruns the one whose parameters have its arguments' types, so annotate an\nargument whose value doesn't show its type (see Values):\n\n{lines}"
    )
}

/// The built-in types, each with its `#` lines from the prelude: "`Text`:
/// A string: …". `Duration`'s are followed by its units.
fn built_in_types() -> String {
    crate::prelude::intrinsics()
        .iter()
        .map(|intrinsic| {
            let mut doc = intrinsic.doc.clone();
            if intrinsic.r#type == Type::Duration {
                let units: Vec<String> = crate::time::units()
                    .iter()
                    .map(|unit| format!("`{}`", unit.singular))
                    .collect();
                let (last, others) = units.split_last().expect("Duration has units");
                doc.push_str(&format!(" The units are {} and {last}.", others.join(", ")));
            }
            let item = wrap(&format!("- `{}`: {doc}", intrinsic.name), "  ");
            match doc.lines().any(str::is_empty) {
                true => format!("{item}\n\n"),
                false => format!("{item}\n"),
            }
        })
        .collect()
}

/// The words of `line`, keeping a `code` span whole so that it doesn't
/// break across lines.
fn words(line: &str) -> Vec<&str> {
    let mut words = Vec::new();
    let mut start = None;
    let mut in_code = false;
    for (index, character) in line.char_indices() {
        match character {
            ' ' if !in_code => {
                if let Some(start) = start.take() {
                    words.push(&line[start..index]);
                }
            }
            _ => {
                if character == '`' {
                    in_code = !in_code;
                }
                start.get_or_insert(index);
            }
        }
    }
    if let Some(start) = start {
        words.push(&line[start..]);
    }
    words
}

/// `text` wrapped to the guide's width, its paragraphs' lines after the
/// first indented by `indent`. A line indented by four spaces, such as an
/// example, is kept as it is, and a blank line stays.
fn wrap(text: &str, indent: &str) -> String {
    const WIDTH: usize = 76;
    let mut lines: Vec<String> = Vec::new();
    let mut paragraph: Vec<&str> = Vec::new();
    let flush = |paragraph: &mut Vec<&str>, lines: &mut Vec<String>| {
        let mut line = match lines.is_empty() {
            true => String::new(),
            false => indent.to_string(),
        };
        let mut empty = true;
        for word in paragraph.drain(..) {
            if !empty && line.chars().count() + 1 + word.chars().count() > WIDTH {
                lines.push(std::mem::replace(&mut line, indent.to_string()));
                empty = true;
            }
            if !empty {
                line.push(' ');
            }
            line.push_str(word);
            empty = false;
        }
        if !empty {
            lines.push(line);
        }
    };
    for line in text.lines() {
        if line.is_empty() || line.starts_with("    ") {
            flush(&mut paragraph, &mut lines);
            lines.push(match line.is_empty() {
                true => String::new(),
                false => format!("{indent}{line}"),
            });
        } else {
            paragraph.extend(words(line));
        }
    }
    flush(&mut paragraph, &mut lines);
    lines.join("\n")
}

/// The rule for generic types, shown with the schema's first one: "`RECORD
/// page of (item: TYPE)`", then a use of it. Empty for a schema without any.
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
    let declared: Vec<String> = definition
        .parameters
        .iter()
        .map(|parameter| format!("{parameter}: TYPE"))
        .collect();
    let arguments = vec!["Text".to_string(); definition.parameters.len()];
    format!(
        "- A generic RECORD or CHOICE has a type parameter in each slot of its\n  name, which its fields or variants use as a type: `{keyword} {}`.\n  A use puts a type in each slot, in parentheses, `{}`, and its value\n  is written with those types in place of the parameters.\n",
        fill_slots(&definition.name, &declared),
        fill_slots(&definition.name, &arguments),
    )
}

/// The rule for named types, and each of them with the type it is written
/// as: "`file id`: `Text`", then its doc. Empty for a schema without any.
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
                definition.name.clone(),
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

const IMPORTS: &str = "

Files form one flat namespace. `USE \"relative path\"` includes a file and
makes its `PUBLIC` types and error groups visible to the importing file.
Imports are direct, with no re-export. File paths never appear in calls or
type names.";

const EXAMPLES: &str = "
`EXAMPLES […]` after a method lists calls of it, each with its result side
by side, as in `read \"a\" RETURNS …`, and a `#` line saying what it shows.
A response writes the result alone.";

/// The schema's files in import traversal order.
fn sources(schema: &Schema) -> String {
    if let [file] = schema.files.as_slice() {
        return file.source.trim_end().to_string();
    }
    schema
        .files
        .iter()
        .map(|file| format!("### `{}`\n\n{}", file.path, file.source.trim_end()))
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
    let mut values = method.shape.parameters.iter().map(|parameter| {
        match always_annotated(schema, &parameter.r#type) {
            true => format!(
                "({} AS: {})",
                sample(schema, &parameter.r#type),
                schema.type_name(&parameter.r#type)
            ),
            false => sample(schema, &parameter.r#type),
        }
    });
    method
        .shape
        .text
        .split(' ')
        .map(|word| match word {
            "_" => values.next().unwrap_or_default(),
            word => word.to_string(),
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Whether no value of `type` shows it, so an argument of it is always
/// annotated: a named type, a generic one, Bytes, Instant, and a list or a
/// map of one of them.
fn always_annotated(schema: &Schema, r#type: &Type) -> bool {
    match r#type {
        Type::Bytes | Type::Instant | Type::Generic(..) | Type::Refined(..) => true,
        Type::Named(index) => matches!(schema.types[*index].kind, TypeKind::Distinct(_)),
        Type::List(item) | Type::Map(item) => always_annotated(schema, item),
        _ => false,
    }
}

/// A value of `type` as an example fills a slot with it.
fn sample(schema: &Schema, r#type: &Type) -> String {
    if crate::refinement::has_refinements(schema, r#type) {
        return crate::refinement::example(schema, r#type)
            .and_then(|value| crate::encode_value(schema, r#type, &value).ok())
            .map(|source| source.trim().to_string())
            .unwrap_or_else(|| "…".to_string());
    }
    match schema.underlying(r#type) {
        Type::Text => "\"a\"".to_string(),
        Type::Bytes => "\"AQI=\"".to_string(),
        Type::Integer => "3".to_string(),
        Type::Float => "1.5".to_string(),
        Type::Boolean => "(true)".to_string(),
        Type::Instant => "\"2026-11-14T06:00:08.000Z\"".to_string(),
        Type::Duration => "(5 minutes)".to_string(),
        Type::Refined(base, _) => sample(schema, base),
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

fn refinements(schema: &Schema) -> String {
    fn describe(schema: &Schema, ty: &Type, label: &str, rows: &mut Vec<String>) {
        match ty {
            Type::Refined(base, predicate) => {
                rows.push(format!(
                    "- `{label}`: a `{}` {} (`{}`).\n",
                    schema.type_name(base),
                    predicate.words(),
                    predicate.source
                ));
                describe(schema, base, label, rows);
            }
            Type::List(item) | Type::Map(item) => {
                describe(schema, item, &format!("{label} item"), rows)
            }
            Type::Generic(_, args) => {
                for arg in args {
                    describe(schema, arg, label, rows);
                }
            }
            _ => {}
        }
    }
    let mut rows = Vec::new();
    for definition in &schema.types {
        match &definition.kind {
            TypeKind::Distinct(base) => describe(schema, base, &definition.name, &mut rows),
            TypeKind::Record(fields) => {
                for f in fields {
                    describe(
                        schema,
                        &f.r#type,
                        &format!("{} / {}", definition.name, f.name),
                        &mut rows,
                    );
                }
            }
            TypeKind::Choice(variants) => {
                for v in variants {
                    for p in &v.shape.parameters {
                        describe(
                            schema,
                            &p.r#type,
                            &format!("{} / {} / {}", definition.name, v.shape.text, p.name),
                            &mut rows,
                        );
                    }
                }
            }
        }
    }
    for method in &schema.methods {
        for p in method.shape.parameters.iter().chain(method.returns.iter()) {
            describe(
                schema,
                &p.r#type,
                &format!("{} / {}", method.shape.text, p.name),
                &mut rows,
            );
        }
    }
    for error in &schema.errors {
        for p in &error.error.shape.parameters {
            describe(
                schema,
                &p.r#type,
                &format!("{} / {}", error.error.shape.text, p.name),
                &mut rows,
            );
        }
    }
    if rows.is_empty() {
        String::new()
    } else {
        format!(
            "## Refinements\n\nA refined value has its base type's representation and must satisfy every\npredicate. A failed argument rejects the whole request before any call runs.\nA failed returned value is an internal server error.\n\n{}\n",
            rows.concat()
        )
    }
}
