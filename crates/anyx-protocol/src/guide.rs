//! The guide a server sends to a client that has never seen the protocol,
//! such as an AI agent: how to write a request and read a response, then the
//! schema as written.

use crate::schema::{Method, Schema, ShapePart, Type, TypeKind, fill_slots};

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
shape of one of the methods below, with a value in each `(name: Type)` slot:

    {example}
{overloads}
Every call runs, in order, and the response has one result per call.
{conditions}
`SCHEMA` is a call every server answers, with this guide, and
`SCHEMA FILES` with the files of the schema, the entry first.

## Effects

Words before `METHOD` say what a call of it may do, and decide when it is
safe to send without asking:

- `READ ONLY METHOD`: it changes nothing, so call it freely. `SCHEMA` and
  `SCHEMA FILES` are read only.
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
  words, put `:` after its key: `` {{ \"hymn\": `done }} ``.
- A record is its fields in braces, each written in its declared shape with
  a value in each slot, in any order. Declare every slot as
  `(name: Type)`. For example, `quiet from (start: Integer) to (end: Integer)`
  is one field, written `{{ quiet from 22 to 7 }}`. Other record values include
  `{{ path \"a\", revision 3 }}`,
  `` {{ (`any) case, between 1 and 3 }} ``. A value before a field's words
  keeps its parentheses. A field declared as a backtick and a slot is written
  as its variant alone: `` {{ `by name, `hidden }} ``. A field with a `DEFAULT` may
  be left out and then holds the entry after `DEFAULT`. When several fields
  share a default variant, leave those fields out to use it. When a value is words
  and the slot is last, put `:` after the field's words:
  `` {{ status: `done }} ``, `{{ timeout: 5 minutes }}`.
- A union of variants, `` TYPE ALIAS Shade [| `dark, `light |] ``, is
  one of its variants, written as a backtick and the variant's shape
  with a value in each slot: `` `dark ``, `` `custom hue 0.5 ``. A call
  without the backtick is a function, such as a named type's constructor.
- A union is a value of one of its members, written as that member
  writes it. So a nullable `T?`, `` [| T, `null |] ``, is `` `null `` or a
  value of `T`: `{{ max depth 3 }}`, `` {{ max depth: `null }} ``.
{generic_types}{named_types}- In a slot, a value written as words or a variant goes in parentheses so
  that it doesn't join the words around it: `` set (`true) ``,
  `` pick (`custom hue 0.5) ``, `wait (5 minutes)`.

An argument whose type its value doesn't show is annotated with `AS:` and
its parameter's type, in parentheses: `tag ([] AS: LIST Text)`. A value
shows its type when it is a number, a string (`Text`), a duration, a list
or map whose items show one type, a named type's value, written with its
name, or a record or variant that only one type of the schema fits. `[]`,
`{{}}`, and a value of a generic type, `Bytes` or `Instant` never do.
Without its annotation, the request is invalid. Only a whole argument is
annotated; a value inside it is read by the type around it. An annotation states the type; it doesn't convert the value.

{prelude}
{refinements}## Responses

The response has one result per call, in the order of the calls, separated
by commas. A result doesn't repeat its call: the first result answers the
first call, the second the second, and so on, so keep a request short
enough to count its results without slipping. A result is one of:

- `RETURNS` and the value the method returns, or `RETURNS NOTHING` for a
  method that returns nothing.
- `ERROR` and one of the method's errors, in the error's shape: a variant
  in its `ERRORS` brackets, or one of the variants of an alias named there.
  For example: `` ERROR `missing ``. The call didn't do anything. Each
  error's `#` lines say what to do next.
- `INTERNAL ERROR`: the server failed. Calls after it don't run.
- `NOT RUN`: the call didn't run, because an earlier call failed that way.

For two calls, one that read a file and one whose file was missing:

    RETURNS {{ path \"a\", revision 3 }},
    ERROR `missing,

A request that can't be read gets one result, and none of its calls run:

    INVALID REQUEST {{ line 1, column 6, message \"…\" }}

## The schema

In the schema, a `#` line before a definition says what it means. Types
are named in Title Case, such as `File Summary`; other names are lowercase
words; all-caps words belong to the schema language. Errors that methods
share are an alias of a union of variants, such as `` TYPE ALIAS File
Errors: [| `missing |] ``, named in each method's `ERRORS`.{examples}{imports}

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
        conditions = if schema.methods.iter().any(|method| method.requires.is_some() || method.ensures.is_some()) {
            "\n`REQUIRES` clauses below must hold for each call's arguments. If one fails,\nthe whole request gets `INVALID REQUEST`. `ENSURES` clauses describe what\nmust hold for a successful result and its call's arguments. A violation\ngets `INTERNAL ERROR`.\n"
        } else {
            ""
        },
        named_types = named_types(schema),
        built_in = built_in_types(),
        prelude = prelude(),
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
        "\nSome methods share a shape. Each call selects the overload its arguments\nfit. One parameter position accepts disjoint types for each pair:\n\n{lines}"
    )
}

/// The built-in types, each with its `#` lines from the prelude: "`Text`:
/// A string: …".
fn built_in_types() -> String {
    crate::prelude::intrinsics()
        .iter()
        .map(|intrinsic| {
            let doc = &intrinsic.doc;
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

/// The rule for generic types, shown with the schema's first one: "`TYPE ALIAS
/// page of (Item: TYPE): { … }`" or "`TYPE ALIAS (Value: TYPE) or (Problem:
/// TYPE) [| … |]`", then a use of it. Empty for a schema without any.
fn generic_types(schema: &Schema) -> String {
    let Some(definition) = schema
        .types
        .iter()
        .find(|definition| !definition.anonymous && !definition.parameters.is_empty())
    else {
        return String::new();
    };
    let (keyword, body) = match definition.kind {
        TypeKind::Variants(_) => ("TYPE ALIAS", " [| … |]"),
        TypeKind::Record(_) => ("TYPE ALIAS", ": { … }"),
        TypeKind::Distinct { .. } => return String::new(),
    };
    let declared: Vec<String> = definition
        .parameters
        .iter()
        .map(|parameter| format!("{parameter}: TYPE"))
        .collect();
    let arguments = vec!["Text".to_string(); definition.parameters.len()];
    format!(
        "- A generic record or type alias has a type parameter in each slot of\n  its name, which its fields or variants use as a type: `{keyword} {}{body}`.\n  A use puts a type in each slot, in parentheses, `{}`, and its value\n  is written with those types in place of the parameters.\n",
        fill_slots(&definition.name, &declared),
        fill_slots(&definition.name, &arguments),
    )
}

/// The rule for named types, and each of them as a value writes it:
/// "`File Id …`, holding a `Text`", then its doc. Empty for a schema
/// without any.
fn named_types(schema: &Schema) -> String {
    let named: Vec<String> = schema
        .types
        .iter()
        .filter_map(|definition| {
            let TypeKind::Distinct {
                constructor,
                underlying,
            } = &definition.kind
            else {
                return None;
            };
            let doc = definition.doc.as_deref().map_or(String::new(), |doc| {
                format!(" {}", doc.split_whitespace().collect::<Vec<_>>().join(" "))
            });
            let slots = crate::schema::slot_types(constructor, underlying)
                .into_iter()
                .map(|slot| crate::schema::code(&schema.type_name(slot)))
                .collect::<Vec<_>>();
            let holding = match slots.as_slice() {
                [] => String::new(),
                [slot] => format!(", holding a {slot}"),
                slots => format!(", holding {}", slots.join(", ")),
            };
            Some(format!(
                "  - `{}`{holding}.{doc}\n",
                ShapePart::text(constructor).replace('_', "…"),
            ))
        })
        .collect();
    match named.is_empty() {
        true => String::new(),
        false => format!(
            "- A NAMED TYPE is a type of its own, written with its name and a\n  value in each slot, as in `File Id \"a\"`:\n{}",
            named.concat()
        ),
    }
}

const IMPORTS: &str = "

Files form one flat namespace. `USE \"relative path\"` includes a file and
makes its `PUBLIC` types visible to the importing file.
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
        None => {
            let call = sample_call(schema, method);
            if method.requires.is_some() && crate::decode_request(schema, &call).is_err() {
                schema.typed_shape(&method.shape)
            } else {
                call
            }
        }
    }
}

/// A call of `method` by its full name, with a sample value in each slot.
fn sample_call(schema: &Schema, method: &Method) -> String {
    let mut values =
        method
            .shape
            .parameters
            .iter()
            .map(|parameter| match always_annotated(&parameter.r#type) {
                true => format!(
                    "({} AS: {})",
                    sample(schema, &parameter.r#type),
                    schema.type_name(&parameter.r#type)
                ),
                false => sample(schema, &parameter.r#type),
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
/// annotated: a generic type, Bytes, Instant, and a list or a
/// map of one of them.
fn always_annotated(r#type: &Type) -> bool {
    match r#type {
        Type::Bytes | Type::Instant | Type::Generic(..) => true,
        Type::Refined(base, _) => always_annotated(base),
        Type::List(item) | Type::Map(item) => always_annotated(item),
        Type::Tuple(slots, _) | Type::Union(slots) => slots.iter().any(always_annotated),
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
        Type::Instant => "\"2026-11-14T06:00:08.000Z\"".to_string(),
        Type::Duration => "(5 minutes)".to_string(),
        Type::Refined(base, _) => sample(schema, base),
        Type::List(_) => "[…]".to_string(),
        Type::Map(_) => "{ … }".to_string(),
        Type::Tuple(slots, _) => slots
            .iter()
            .map(|slot| sample(schema, slot))
            .collect::<Vec<_>>()
            .join(" "),
        Type::Named(index) | Type::Generic(index, _) => match &schema.types[*index].kind {
            TypeKind::Record(_) => "{ … }".to_string(),
            TypeKind::Variants(_) => "(…)".to_string(),
            TypeKind::Distinct {
                constructor,
                underlying,
            } => {
                let mut slots = crate::schema::slot_types(constructor, underlying).into_iter();
                let written = constructor
                    .iter()
                    .map(|part| match part {
                        ShapePart::Word(word) => word.clone(),
                        ShapePart::Slot => slots
                            .next()
                            .map_or("…".to_string(), |slot| sample(schema, slot)),
                    })
                    .collect::<Vec<_>>()
                    .join(" ");
                format!("({written})")
            }
        },
        Type::Parameter(_) => "…".to_string(),
        Type::Union(members) => members
            .first()
            .map_or("…".to_string(), |member| sample(schema, member)),
        Type::Variant(shape) => format!("({})", schema.shape_name(shape)),
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
            Type::Tuple(slots, _) => {
                for (index, slot) in slots.iter().enumerate() {
                    describe(schema, slot, &format!("{label} slot {}", index + 1), rows);
                }
            }
            _ => {}
        }
    }
    let mut rows = Vec::new();
    for definition in &schema.types {
        match &definition.kind {
            TypeKind::Distinct {
                underlying: base, ..
            } => describe(schema, base, &definition.name, &mut rows),
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
            TypeKind::Variants(variants) => {
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
    // An error an alias shares is among the errors of each method naming it.
    let mut described: Vec<&str> = Vec::new();
    for error in schema.methods.iter().flat_map(|method| &method.errors) {
        if described.contains(&error.shape.text.as_str()) {
            continue;
        }
        described.push(&error.shape.text);
        for p in &error.shape.parameters {
            describe(
                schema,
                &p.r#type,
                &format!("{} / {}", error.shape.text, p.name),
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

/// Show the ordinary prelude library, including its docs and variant shapes.
fn prelude() -> String {
    let source = crate::prelude::declarations()
        .definition
        .iter()
        .map(|definition| {
            let span = definition.span();
            format!("{},", &crate::prelude::SOURCE[span.start..span.end])
        })
        .collect::<Vec<_>>()
        .join("\n\n");
    format!(
        "## Prelude\n\nThese ordinary types are visible in every schema. `T?` is nullable; only\n`DEFAULT` makes a record field omittable.\n\n```anyx\n{source}\n```\n"
    )
}
