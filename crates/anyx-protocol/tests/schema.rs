//! Reading and checking schemas.

mod common;

use anyx_protocol::Value;
use anyx_protocol::schema::{Type, TypeKind};
use common::{assert_snapshot, diagnostics, poentaro, read, schema};

#[test]
fn the_schema_decoder_is_what_its_grammar_generates() {
    let grammar = read("protocol.anyx");
    let output = anyx_generator::generate(&grammar).unwrap_or_else(|errors| {
        let errors: Vec<String> = errors
            .iter()
            .map(|error| error.render(&grammar, "protocol.anyx"))
            .collect();
        panic!("{}", errors.join("\n"))
    });
    let warnings: Vec<String> = output
        .warnings
        .iter()
        .map(|warning| warning.render(&grammar, "protocol.anyx"))
        .collect();
    assert_eq!(warnings, Vec::<String>::new());
    let code = anyx_generator::format(&output.code).expect("rustfmt is installed");
    assert_snapshot("src/grammar.rs", &code);
}

#[test]
fn poentaros_schema_is_formatted() {
    let path = "tests/fixtures/poentaro.protocol.anyx";
    let formatted = anyx_protocol::format_schema(&common::read(path)).unwrap();
    common::assert_snapshot(path, &formatted);
}

#[test]
fn poentaros_schema_reads_with_its_docs_shapes_and_errors() {
    let schema = poentaro();
    assert_eq!(schema.name, "poentaro");
    assert_eq!(schema.version, 1);
    assert!(schema.doc.as_deref().unwrap().starts_with("Poentaro turns"));
    let shapes: Vec<&str> = schema
        .methods
        .iter()
        .map(|method| method.shape.text.as_str())
        .collect();
    assert_eq!(
        shapes,
        [
            "list files",
            "read _",
            "write _ at revision _ content _",
            "create _ content _",
            "export midi _"
        ]
    );
    let write = schema.method("write _ at revision _ content _").unwrap();
    assert_eq!(write.shape.words(), "write at revision content");
    assert_eq!(
        write.doc.as_deref(),
        Some("Saves the score, if the file is still at `revision`.")
    );
    let parameters: Vec<(&str, &Type)> = write
        .shape
        .parameters
        .iter()
        .map(|parameter| (parameter.name.as_str(), &parameter.r#type))
        .collect();
    assert_eq!(
        parameters,
        [
            ("path", &Type::Text),
            ("revision", &Type::Integer),
            ("content", &Type::Text)
        ]
    );
    assert_eq!(write.returns.as_ref().unwrap().name, "new revision");
    assert_eq!(write.returns.as_ref().unwrap().r#type, Type::Integer);
    let errors: Vec<&str> = schema
        .errors_of(write)
        .iter()
        .map(|error| error.shape.text.as_str())
        .collect();
    assert_eq!(
        errors,
        [
            "conflict at revision _ content _",
            "rejected _",
            "trashed",
            "missing"
        ]
    );
    let list = schema.method("list files").unwrap();
    assert_eq!(
        schema.type_name(&list.returns.as_ref().unwrap().r#type),
        "LIST File Summary"
    );
    assert!(list.errors.is_empty());
    let conflict = schema
        .errors
        .iter()
        .find(|error| error.error.shape.words() == "conflict at revision content")
        .unwrap();
    assert_eq!(
        conflict.error.doc.as_deref(),
        Some("Someone saved first. Apply your change to `content`, then write at\n`revision`.")
    );
}

#[test]
fn choice_variants_are_shapes_with_docs() {
    let schema = schema(
        "PROTOCOL a VERSION 1,
        CHOICE mode {
            dark,
            # A colour of the wheel.
            custom hue (hue: Float) lightness (lightness: Float),
            # Plain.
            plain,
            (base: Text) tinted,
        },",
    );
    let TypeKind::Choice(variants) = &schema.types[0].kind else {
        panic!("mode is a choice")
    };
    let shapes: Vec<(&str, Option<&str>)> = variants
        .iter()
        .map(|variant| (variant.shape.text.as_str(), variant.doc.as_deref()))
        .collect();
    assert_eq!(
        shapes,
        [
            ("dark", None),
            ("custom hue _ lightness _", Some("A colour of the wheel.")),
            ("plain", Some("Plain.")),
            ("_ tinted", None),
        ]
    );
}

#[test]
fn optional_fields_and_record_inputs_read() {
    let schema = schema(
        "PROTOCOL shop VERSION 2,
        RECORD filter { name: OPTIONAL Text, limit: OPTIONAL Integer },
        METHOD find (filter: filter) RETURNS (names: LIST Text),",
    );
    let TypeKind::Record(fields) = &schema.types[0].kind else {
        panic!("filter is a record")
    };
    assert!(fields.iter().all(|field| field.optional));
    assert_eq!(schema.methods[0].shape.parameters[0].r#type, Type::Named(0));
}

#[test]
fn mistakes_in_a_schema_are_diagnosed() {
    let cases = [
        (
            "an unknown type",
            "PROTOCOL a VERSION 1, METHOD f (x: thing) RETURNS (y: Text),",
        ),
        (
            "a built-in type under its old all-caps name",
            "PROTOCOL a VERSION 1, METHOD f (x: INTEGER) RETURNS (y: Text),",
        ),
        (
            "an old all-caps name inside a list",
            "PROTOCOL a VERSION 1, RECORD x { tags: LIST: TEXT },",
        ),
        (
            "INTRINSIC in a schema",
            "PROTOCOL a VERSION 1, INTRINSIC TYPE Money: Integer,",
        ),
        (
            "INTRINSIC on a record in a schema",
            "PROTOCOL a VERSION 1, INTRINSIC RECORD x { a: Text },",
        ),
        (
            "INTRINSIC on a method",
            "PROTOCOL a VERSION 1, INTRINSIC METHOD f RETURNS (y: Text),",
        ),
        (
            "a type named like a built-in one",
            "PROTOCOL a VERSION 1, RECORD Integer { a: Text },",
        ),
        (
            "a named type without a type",
            "PROTOCOL a VERSION 1, TYPE Money,",
        ),
        (
            "an unnamed return",
            "PROTOCOL a VERSION 1, METHOD f RETURNS (Text),",
        ),
        (
            "a bad error after RETURNS NOTHING, which is not an unnamed return",
            "PROTOCOL a VERSION 1, METHOD f RETURNS NOTHING ERRORS [gone 3],",
        ),
        (
            "a type defined twice",
            "PROTOCOL a VERSION 1, RECORD x { a: Text }, CHOICE x { b },",
        ),
        (
            "a field twice",
            "PROTOCOL a VERSION 1, RECORD x { a: Text, a: Integer },",
        ),
        (
            "a variant twice",
            "PROTOCOL a VERSION 1, CHOICE x { a, a (b: Text) },",
        ),
        (
            "a parameter twice",
            "PROTOCOL a VERSION 1, METHOD f (x: Text) g (x: Text) RETURNS (y: Text),",
        ),
        (
            "a method twice",
            "PROTOCOL a VERSION 1, METHOD f RETURNS (y: Text), METHOD f RETURNS (z: Text),",
        ),
        (
            "overloads with the same parameter types",
            "PROTOCOL a VERSION 1, TYPE id: Text, METHOD tag (names: LIST Text) RETURNS NOTHING, METHOD tag (ids: LIST: id) RETURNS NOTHING, METHOD tag (other: LIST Text) RETURNS NOTHING,",
        ),
        (
            "overloads that differ only in what they return",
            "PROTOCOL a VERSION 1, METHOD scale (by: Float) RETURNS (scaled: Float), METHOD scale (by: Float) RETURNS (scaled: Integer),",
        ),
        (
            "a type holding itself",
            "PROTOCOL a VERSION 1, RECORD node { next: node }, RECORD tree { children: LIST: tree },",
        ),
        (
            "a type holding itself through an optional field",
            "PROTOCOL a VERSION 1, RECORD node { next: OPTIONAL: node }, RECORD tree { children: MAP: tree },",
        ),
        (
            "a map default that is not a map",
            "PROTOCOL a VERSION 1, RECORD x { a: OPTIONAL: MAP: Integer DEFAULT [] },",
        ),
        (
            "a cycle through a choice",
            "PROTOCOL a VERSION 1, RECORD a { b: b }, CHOICE b { has (a: a), none },",
        ),
        (
            "names that are not lowercase ASCII",
            "PROTOCOL a VERSION 1, RECORD x { Path: Text, café: Text },",
        ),
        (
            "a parameter named kind",
            "PROTOCOL a VERSION 1, CHOICE x { y (kind: Text) },",
        ),
        (
            "a keyword in a name",
            "PROTOCOL a VERSION 1, RECORD x { a Text b: Text },",
        ),
        (
            "an unknown group",
            "PROTOCOL a VERSION 1, METHOD f RETURNS (y: Text) ERRORS gone,",
        ),
        (
            "an error where a group goes",
            "PROTOCOL a VERSION 1, ERRORS x [gone], METHOD f RETURNS (y: Text) ERRORS [lost] + gone,",
        ),
        (
            "groups joining each other",
            "PROTOCOL a VERSION 1, ERRORS x [a] + y, ERRORS y [b] + x,",
        ),
        (
            "an error listed twice through a group",
            "PROTOCOL a VERSION 1, ERRORS x [gone], METHOD f RETURNS (y: Text) ERRORS x + x,",
        ),
        (
            "an error defined twice in a scope",
            "PROTOCOL a VERSION 1, ERRORS x [gone], METHOD f RETURNS (y: Text) ERRORS [gone],",
        ),
        (
            "a group joining an unknown group",
            "PROTOCOL a VERSION 1, ERRORS x [a] + lost, METHOD f RETURNS (y: Text) ERRORS x,",
        ),
        (
            "a group without a name",
            "PROTOCOL a VERSION 1, ERRORS [gone],",
        ),
        (
            "a group defined twice",
            "PROTOCOL a VERSION 1, ERRORS x [a], ERRORS x [b],",
        ),
        (
            "a name before a method's errors",
            "PROTOCOL a VERSION 1, METHOD f RETURNS (y: Text) ERRORS mine [gone],",
        ),
        (
            "errors in brackets after a group",
            "PROTOCOL a VERSION 1, ERRORS x [a], METHOD f RETURNS (y: Text) ERRORS x + [b],",
        ),
        (
            "an operator other than `+` among errors",
            "PROTOCOL a VERSION 1, ERRORS x [a], METHOD f RETURNS (y: Text) ERRORS [b] - x,",
        ),
        (
            "an error defined on its own",
            "PROTOCOL a VERSION 1, ERROR gone,",
        ),
        (
            "a string inside an error",
            "PROTOCOL a VERSION 1, METHOD f RETURNS (y: Text) ERRORS [gone (a: Text) # doc\n],",
        ),
        (
            "a string inside a variant",
            "PROTOCOL a VERSION 1, CHOICE x { y (a: Text) # doc\n},",
        ),
    ];
    let rendered: Vec<String> = cases
        .iter()
        .map(|(title, source)| format!("== {title}\n{source}\n{}\n", diagnostics(source)))
        .collect();
    assert_snapshot("tests/snapshots/diagnostics.txt", &rendered.join("\n"));
}

#[test]
fn a_doc_is_a_line_string() {
    let cases = [
        (
            "a quoted doc on the protocol",
            "\"A protocol.\" PROTOCOL a VERSION 1,",
        ),
        (
            "a quoted doc on a method",
            "PROTOCOL a VERSION 1,\n\"Moves a file.\" METHOD move (path: Text) to (new path: Text) RETURNS NOTHING,",
        ),
        (
            "a raw doc on a method",
            "PROTOCOL a VERSION 1,\n\"\"\"\nMoves a file.\n\"\"\" METHOD move (path: Text) RETURNS NOTHING,",
        ),
        (
            "a quoted doc on a record",
            "PROTOCOL a VERSION 1, \"A file.\" RECORD file { path: Text },",
        ),
        (
            "a quoted doc on a field",
            "PROTOCOL a VERSION 1, RECORD file { \"Where it is.\" path: Text },",
        ),
        (
            "a quoted doc on a choice",
            "PROTOCOL a VERSION 1, \"A shade.\" CHOICE shade { dark, light },",
        ),
        (
            "a quoted doc on a variant",
            "PROTOCOL a VERSION 1, CHOICE shade { \"Dim.\" dark, light },",
        ),
        (
            "a quoted doc on a type",
            "PROTOCOL a VERSION 1, \"A path.\" TYPE path: Text,",
        ),
        (
            "a quoted doc on a group of errors",
            "PROTOCOL a VERSION 1, \"Shared.\" ERRORS file errors [missing],",
        ),
        (
            "a quoted doc on an error",
            "PROTOCOL a VERSION 1, METHOD f RETURNS (y: Text) ERRORS [\"Gone.\" missing],",
        ),
    ];
    let rendered: Vec<String> = cases
        .iter()
        .map(|(title, source)| format!("== {title}\n{source}\n{}\n", diagnostics(source)))
        .collect();
    assert_snapshot(
        "tests/snapshots/quoted_doc_diagnostics.txt",
        &rendered.join("\n"),
    );
}

#[test]
fn the_example_in_protocol_md_reads() {
    let document = common::read("PROTOCOL.md");
    let example = document
        .split("```anyx\n")
        .nth(1)
        .and_then(|rest| rest.split("```").next())
        .unwrap();
    let schema = common::schema(example);
    assert_eq!(
        schema
            .errors_of(schema.method("write _ at revision _ content _").unwrap())
            .len(),
        4
    );
    assert_eq!(anyx_protocol::format_schema(example).unwrap(), example);
}

#[test]
fn a_method_may_return_nothing() {
    let schema = schema(
        "PROTOCOL a VERSION 1,
        METHOD sign out RETURNS NOTHING,
        METHOD trash (path: Text) RETURNS NOTHING ERRORS [missing],",
    );
    let sign_out = schema.method("sign out").unwrap();
    assert_eq!(sign_out.returns, None);
    assert!(sign_out.errors.is_empty());
    let trash = schema.method("trash _").unwrap();
    assert_eq!(trash.returns, None);
    assert_eq!(trash.errors.len(), 1);
}

#[test]
fn an_optional_field_may_have_a_default() {
    let schema = schema(
        "PROTOCOL a VERSION 1,
        CHOICE shade { dark, light },
        RECORD page { size: OPTIONAL: Integer DEFAULT 50, after: OPTIONAL Text },
        RECORD search {
            query: Text,
            limit: OPTIONAL: Integer DEFAULT 50,
            include trash: OPTIONAL: Boolean DEFAULT false,
            shade: OPTIONAL: shade DEFAULT dark,
            tags: OPTIONAL: LIST: Text DEFAULT [\"new\"],
            shades: OPTIONAL: LIST shade DEFAULT [light],
            sizes: OPTIONAL: (LIST Integer) DEFAULT [],
            wait: OPTIONAL: Duration DEFAULT 5 minutes,
            page: OPTIONAL: page DEFAULT { after \"a\" },
            note: OPTIONAL Text,
        },",
    );
    let TypeKind::Record(fields) = &schema.types[2].kind else {
        panic!("search is a record")
    };
    let defaults: Vec<(&str, Option<&Value>)> = fields
        .iter()
        .map(|field| (field.name.as_str(), field.default.as_ref()))
        .collect();
    let text = |text: &str| Value::Text(text.to_string());
    let variant = |shape: &str| Value::Variant {
        shape: shape.to_string(),
        arguments: vec![],
    };
    let page = Value::Record(
        [
            ("after".to_string(), text("a")),
            ("size".to_string(), Value::Integer(50)),
        ]
        .into(),
    );
    assert_eq!(
        defaults,
        [
            ("query", None),
            ("limit", Some(&Value::Integer(50))),
            ("include trash", Some(&Value::Boolean(false))),
            ("shade", Some(&variant("dark"))),
            ("tags", Some(&Value::List(vec![text("new")]))),
            ("shades", Some(&Value::List(vec![variant("light")]))),
            ("sizes", Some(&Value::List(vec![]))),
            (
                "wait",
                Some(&Value::Duration(std::time::Duration::from_secs(300)))
            ),
            ("page", Some(&page)),
            ("note", None),
        ]
    );
    let types: Vec<String> = fields
        .iter()
        .map(|field| schema.type_name(&field.r#type))
        .collect();
    assert_eq!(
        types,
        [
            "Text",
            "Integer",
            "Boolean",
            "shade",
            "LIST Text",
            "LIST shade",
            "LIST Integer",
            "Duration",
            "page",
            "Text"
        ]
    );
    assert!(fields.iter().skip(1).all(|field| field.optional));
}

#[test]
fn mistakes_in_a_default_are_diagnosed() {
    let cases = [
        (
            "a default of another type",
            "PROTOCOL a VERSION 1, RECORD x { limit: OPTIONAL: Integer DEFAULT \"50\" },",
        ),
        (
            "a default that is no variant",
            "PROTOCOL a VERSION 1, CHOICE shade { dark }, RECORD x { shade: OPTIONAL: shade DEFAULT dim },",
        ),
        (
            "a default on a field that must be written",
            "PROTOCOL a VERSION 1, RECORD x { limit: Integer DEFAULT 50 },",
        ),
        (
            "a default on a parameter",
            "PROTOCOL a VERSION 1, METHOD f (limit: Integer DEFAULT 50) RETURNS (y: Text),",
        ),
    ];
    let rendered: Vec<String> = cases
        .iter()
        .map(|(title, source)| format!("== {title}\n{source}\n{}\n", diagnostics(source)))
        .collect();
    assert_snapshot(
        "tests/snapshots/default_diagnostics.txt",
        &rendered.join("\n"),
    );
}

#[test]
fn a_schema_with_defaults_is_formatted_as_written() {
    let source = "PROTOCOL a VERSION 1,

RECORD search { limit: OPTIONAL: Integer DEFAULT 50, tags: OPTIONAL: LIST: Text DEFAULT [] },
";
    assert_eq!(anyx_protocol::format_schema(source).unwrap(), source);
}

#[test]
fn a_map_holds_values_by_text_and_may_hold_its_own_type() {
    let schema = schema(
        "PROTOCOL a VERSION 1,
        CHOICE shade { dark, light },
        RECORD folder {
            files: MAP Integer,
            folders: MAP: folder,
            shades: OPTIONAL: MAP: shade DEFAULT { \"a\": dark },
            names: OPTIONAL: MAP: LIST: Text DEFAULT { \"b\" [\"c\"] },
            lights: OPTIONAL: MAP shade DEFAULT {},
        },
        METHOD sizes (sizes: MAP Float) RETURNS (folder: folder),",
    );
    let TypeKind::Record(fields) = &schema.types[1].kind else {
        panic!("folder is a record")
    };
    let types: Vec<String> = fields
        .iter()
        .map(|field| schema.type_name(&field.r#type))
        .collect();
    assert_eq!(
        types,
        [
            "MAP Integer",
            "MAP folder",
            "MAP shade",
            "MAP: LIST Text",
            "MAP shade"
        ]
    );
    let variant = |shape: &str| Value::Variant {
        shape: shape.to_string(),
        arguments: vec![],
    };
    assert_eq!(
        fields[2].default,
        Some(Value::Map([("a".to_string(), variant("dark"))].into()))
    );
    assert_eq!(
        fields[3].default,
        Some(Value::Map(
            [(
                "b".to_string(),
                Value::List(vec![Value::Text("c".to_string())])
            )]
            .into()
        ))
    );
    assert_eq!(fields[4].default, Some(Value::Map([].into())));
    assert_eq!(
        schema.methods[0].shape.parameters[0].r#type,
        Type::Map(Box::new(Type::Float))
    );
}
