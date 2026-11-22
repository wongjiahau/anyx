//! Reading and checking schemas.

mod common;

use anyx_protocol::Value;
use anyx_protocol::schema::{Type, TypeKind};
use common::{assert_snapshot, diagnostics, poentaro, read, schema};

#[test]
fn the_schema_decoder_is_what_its_grammar_generates() {
    let grammar = read("protocol.grammar.anyx");
    let output = anyx_generator::generate(&grammar).unwrap_or_else(|errors| {
        let errors: Vec<String> = errors
            .iter()
            .map(|error| error.render(&grammar, "protocol.grammar.anyx"))
            .collect();
        panic!("{}", errors.join("\n"))
    });
    let warnings: Vec<String> = output
        .warnings
        .iter()
        .map(|warning| warning.render(&grammar, "protocol.grammar.anyx"))
        .collect();
    // A named slot and a bare type can both be written as a lowercase call.
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert!(
        warnings[0].contains("choices `parameter` and `type`"),
        "{warnings:?}"
    );
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
    let conflict = write
        .errors
        .iter()
        .find(|error| error.shape.words() == "conflict at revision content")
        .unwrap();
    assert_eq!(
        conflict.doc.as_deref(),
        Some("Someone saved first. Apply your change to `content`, then write at\n`revision`.")
    );
}

#[test]
fn a_choice_names_the_union_that_replaced_it() {
    let message = diagnostics("PROTOCOL a VERSION 1, CHOICE Case { `smart, `any },");
    assert!(
        message.contains("expected `TYPE ALIAS Case [|"),
        "{message}"
    );
}

#[test]
fn a_union_member_may_have_a_doc_before_it() {
    schema(
        "PROTOCOL a VERSION 1,
        NAMED TYPE File Id (value: Text),
        TYPE ALIAS Target [|
            # A file by its id.
            File Id,
            # Nothing at all.
            `none,
        |],",
    );
    let message = diagnostics(
        "PROTOCOL a VERSION 1, TYPE ALIAS Target [| Integer # A count.
        |],",
    );
    assert!(
        message.contains(
            "found a doc among the types of a union's member; expected it before the member"
        ),
        "{message}"
    );
}

#[test]
fn a_union_of_variants_cannot_lead_back_to_itself() {
    for source in [
        "PROTOCOL a VERSION 1, TYPE ALIAS Tree [| `leaf, `node (children: LIST Tree) |],",
        "PROTOCOL a VERSION 1, TYPE ALIAS Alpha [| `beta (b: Beta) |], TYPE ALIAS Beta [| `alpha (a: MAP Alpha), `none |],",
    ] {
        let message = diagnostics(source);
        assert!(
            message.contains("a union of variants that leads back to itself"),
            "{message}"
        );
    }
}

#[test]
fn variants_are_shapes_with_docs() {
    let schema = schema(
        "PROTOCOL a VERSION 1,
        TYPE ALIAS Mode [|
            `dark,
            # A colour of the wheel.
            `custom hue (hue: Float) lightness (lightness: Float),
            # Plain.
            `plain,
            `(base: Text) tinted,
        |],",
    );
    let TypeKind::Variants(variants) = &schema.types[0].kind else {
        panic!("mode is a union of variants")
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
        TYPE Filter { name (Text) DEFAULT name \"\", limit (Integer) DEFAULT limit 0 },
        METHOD find (filter: Filter) RETURNS (names: LIST Text) ERRORS [| |],",
    );
    let TypeKind::Record(fields) = &schema.types[0].kind else {
        panic!("filter is a record")
    };
    assert!(fields.iter().all(|field| field.default.is_some()));
    assert_eq!(schema.methods[0].shape.parameters[0].r#type, Type::Named(0));
}

#[test]
fn mistakes_in_a_schema_are_diagnosed() {
    let cases = [
        (
            "an unknown type",
            "PROTOCOL a VERSION 1, METHOD f (x: thing) RETURNS (y: Text) ERRORS [| |],",
        ),
        (
            "a built-in type under its old all-caps name",
            "PROTOCOL a VERSION 1, METHOD f (x: INTEGER) RETURNS (y: Text) ERRORS [| |],",
        ),
        (
            "an old all-caps name inside a list",
            "PROTOCOL a VERSION 1, TYPE Example { tags: LIST: TEXT },",
        ),
        (
            "INTRINSIC in a schema",
            "PROTOCOL a VERSION 1, INTRINSIC TYPE Money,",
        ),
        (
            "INTRINSIC on a record in a schema",
            "PROTOCOL a VERSION 1, INTRINSIC TYPE Example { a: Text },",
        ),
        (
            "INTRINSIC on a method",
            "PROTOCOL a VERSION 1, INTRINSIC METHOD f RETURNS (y: Text) ERRORS [| |],",
        ),
        (
            "a type named like a built-in one",
            "PROTOCOL a VERSION 1, TYPE Integer { a: Text },",
        ),
        (
            "a type alias without a type",
            "PROTOCOL a VERSION 1, TYPE ALIAS Money,",
        ),
        (
            "an unnamed return",
            "PROTOCOL a VERSION 1, METHOD f RETURNS (Text) ERRORS [| |],",
        ),
        (
            "a bad error after RETURNS NOTHING, which is not an unnamed return",
            "PROTOCOL a VERSION 1, METHOD f RETURNS NOTHING ERRORS [| `gone 3 |],",
        ),
        (
            "a type defined twice",
            "PROTOCOL a VERSION 1, TYPE Example { a: Text }, TYPE ALIAS Example [| `b |],",
        ),
        (
            "a field twice",
            "PROTOCOL a VERSION 1, TYPE Example { a: Text, a: Integer },",
        ),
        (
            "a variant twice",
            "PROTOCOL a VERSION 1, TYPE ALIAS Example [| `a, `a (b: Text) |],",
        ),
        (
            "a parameter twice",
            "PROTOCOL a VERSION 1, METHOD f (x: Text) g (x: Text) RETURNS (y: Text) ERRORS [| |],",
        ),
        (
            "a method twice",
            "PROTOCOL a VERSION 1, METHOD f RETURNS (y: Text) ERRORS [| |], METHOD f RETURNS (z: Text) ERRORS [| |],",
        ),
        (
            "overloads with the same parameter types",
            "PROTOCOL a VERSION 1, NAMED TYPE Id (value: Text), METHOD tag (names: LIST Text) RETURNS NOTHING ERRORS [| |], METHOD tag (ids: LIST: Id) RETURNS NOTHING ERRORS [| |], METHOD tag (other: LIST Text) RETURNS NOTHING ERRORS [| |],",
        ),
        (
            "overloads that differ only in what they return",
            "PROTOCOL a VERSION 1, METHOD scale (by: Float) RETURNS (scaled: Float) ERRORS [| |], METHOD scale (by: Float) RETURNS (scaled: Integer) ERRORS [| |],",
        ),
        (
            "a type holding itself",
            "PROTOCOL a VERSION 1, TYPE Node { next: Node }, TYPE Tree { children: LIST: Tree },",
        ),
        (
            "a type holding itself through an optional field",
            "PROTOCOL a VERSION 1, TYPE Node { next (Node?) DEFAULT next (`null) }, TYPE Tree { children: MAP: Tree },",
        ),
        (
            "a map default that is not a map",
            "PROTOCOL a VERSION 1, TYPE Example { a (MAP: Integer) DEFAULT a [] },",
        ),
        (
            "a cycle through a union of variants",
            "PROTOCOL a VERSION 1, TYPE Alpha { b: Beta }, TYPE ALIAS Beta [| `has (a: Alpha), `none |],",
        ),
        (
            "names that are not lowercase ASCII",
            "PROTOCOL a VERSION 1, TYPE Example { Path: Text, café: Text },",
        ),
        (
            "a parameter named kind",
            "PROTOCOL a VERSION 1, TYPE ALIAS Example [| `y (kind: Text) |],",
        ),
        (
            "a keyword in a name",
            "PROTOCOL a VERSION 1, TYPE Example { a Text b: Text },",
        ),
        (
            "a method without errors",
            "PROTOCOL a VERSION 1, METHOD f RETURNS (y: Text),",
        ),
        (
            "an unknown alias among errors",
            "PROTOCOL a VERSION 1, METHOD f RETURNS (y: Text) ERRORS [| Gone |],",
        ),
        (
            "a type among errors",
            "PROTOCOL a VERSION 1, METHOD f RETURNS (y: Text) ERRORS [| Integer, [| `a, `b |] |],",
        ),
        (
            "an alias of other types among errors",
            "PROTOCOL a VERSION 1, TYPE ALIAS Maybe [| Text, `none |], METHOD f RETURNS (y: Text) ERRORS [| Maybe |],",
        ),
        (
            "aliases including each other",
            "PROTOCOL a VERSION 1, TYPE ALIAS Read Errors [| `a, Write Errors |], TYPE ALIAS Write Errors [| `b, Read Errors |], METHOD f RETURNS (y: Text) ERRORS [| Read Errors |],",
        ),
        (
            "an error listed twice through an alias",
            "PROTOCOL a VERSION 1, TYPE ALIAS Shared [| `gone |], METHOD f RETURNS (y: Text) ERRORS [| Shared, Shared |],",
        ),
        (
            "an alias including an unknown alias",
            "PROTOCOL a VERSION 1, TYPE ALIAS Shared [| `a, Lost |], METHOD f RETURNS (y: Text) ERRORS [| Shared |],",
        ),
        (
            "a group of errors",
            "PROTOCOL a VERSION 1, ERRORS x [`a] + y,",
        ),
        (
            "errors outside a method",
            "PROTOCOL a VERSION 1, ERRORS [| `gone |],",
        ),
        (
            "a name before a method's errors",
            "PROTOCOL a VERSION 1, METHOD f RETURNS (y: Text) ERRORS mine [| `gone |],",
        ),
        (
            "an operator after a method's errors",
            "PROTOCOL a VERSION 1, TYPE ALIAS Shared [| `a |], METHOD f RETURNS (y: Text) ERRORS [| `b |] + Shared,",
        ),
        (
            "an error defined on its own",
            "PROTOCOL a VERSION 1, ERROR `gone,",
        ),
        (
            "a string inside an error",
            "PROTOCOL a VERSION 1, METHOD f RETURNS (y: Text) ERRORS [| `gone (a: Text) # doc\n|],",
        ),
        (
            "a string inside a variant",
            "PROTOCOL a VERSION 1, TYPE ALIAS Example [| `y (a: Text) # doc\n|],",
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
            "PROTOCOL a VERSION 1,\n\"Moves a file.\" METHOD move (path: Text) to (new path: Text) RETURNS NOTHING ERRORS [| |],",
        ),
        (
            "a raw doc on a method",
            "PROTOCOL a VERSION 1,\n\"\"\"\nMoves a file.\n\"\"\" METHOD move (path: Text) RETURNS NOTHING ERRORS [| |],",
        ),
        (
            "a quoted doc on a record",
            "PROTOCOL a VERSION 1, \"A file.\" TYPE File { path: Text },",
        ),
        (
            "a quoted doc on a field",
            "PROTOCOL a VERSION 1, TYPE File { \"Where it is.\" path: Text },",
        ),
        (
            "a quoted doc on a choice",
            "PROTOCOL a VERSION 1, \"A shade.\" TYPE ALIAS Shade [| `dark, `light |],",
        ),
        (
            "a quoted doc on a variant",
            "PROTOCOL a VERSION 1, TYPE ALIAS Shade [| `\"Dim.\" dark, light |],",
        ),
        (
            "a quoted doc on a type",
            "PROTOCOL a VERSION 1, \"A path.\" TYPE ALIAS Path: Text,",
        ),
        (
            "a quoted doc on an error",
            "PROTOCOL a VERSION 1, METHOD f RETURNS (y: Text) ERRORS [| `\"Gone.\" missing |],",
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
        METHOD sign out RETURNS NOTHING ERRORS [| |],
        METHOD trash (path: Text) RETURNS NOTHING ERRORS [| `missing |],",
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
        TYPE ALIAS Shade [| `dark, `light |],
        TYPE Page { size (Integer) DEFAULT size 50, after (Text) DEFAULT after \"\" },
        TYPE Search {
            query: Text,
            limit (Integer) DEFAULT limit 50,
            include trash (Boolean) DEFAULT include trash (`false),
            shade (Shade) DEFAULT shade (`dark),
            tags (LIST: Text) DEFAULT tags [\"new\"],
            shades (LIST Shade) DEFAULT shades [`light],
            sizes (LIST Integer) DEFAULT sizes [],
            wait (Duration) DEFAULT wait (5 minutes),
            page (Page) DEFAULT page { after \"a\" },
            note (Text) DEFAULT note \"\",
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
            (
                "include trash",
                Some(&Value::Variant {
                    shape: "false".into(),
                    arguments: vec![]
                })
            ),
            ("shade", Some(&variant("dark"))),
            ("tags", Some(&Value::List(vec![text("new")]))),
            ("shades", Some(&Value::List(vec![variant("light")]))),
            ("sizes", Some(&Value::List(vec![]))),
            (
                "wait",
                Some(&Value::Duration(std::time::Duration::from_secs(300)))
            ),
            ("page", Some(&page)),
            ("note", Some(&text(""))),
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
            "Shade",
            "LIST Text",
            "LIST Shade",
            "LIST Integer",
            "Duration",
            "Page",
            "Text"
        ]
    );
    assert!(fields.iter().skip(1).all(|field| field.default.is_some()));
}

#[test]
fn mistakes_in_a_default_are_diagnosed() {
    let cases = [
        (
            "a default of another type",
            "PROTOCOL a VERSION 1, TYPE Example { limit (Integer) DEFAULT limit \"50\" },",
        ),
        (
            "a default that is no variant",
            "PROTOCOL a VERSION 1, TYPE ALIAS Shade [| `dark |], TYPE Example { shade (Shade) DEFAULT shade (`dim) },",
        ),
        (
            "the removed OPTIONAL keyword",
            "PROTOCOL a VERSION 1, TYPE Example { limit: OPTIONAL Integer },",
        ),
        (
            "a default on a parameter",
            "PROTOCOL a VERSION 1, METHOD f (limit: Integer DEFAULT 50) RETURNS (y: Text) ERRORS [| |],",
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

TYPE Search { limit (Integer) DEFAULT limit 50, tags (LIST Text) DEFAULT tags [] },
";
    assert_eq!(anyx_protocol::format_schema(source).unwrap(), source);
}

#[test]
fn a_map_holds_values_by_text_and_may_hold_its_own_type() {
    let schema = schema(
        "PROTOCOL a VERSION 1,
        TYPE ALIAS Shade [| `dark, `light |],
        TYPE Folder {
            files: MAP Integer,
            folders: MAP: Folder,
            shades (MAP: Shade) DEFAULT shades { \"a\": `dark },
            names (MAP: LIST: Text) DEFAULT names { \"b\" [\"c\"] },
            lights (MAP Shade) DEFAULT lights {},
        },
        METHOD sizes (sizes: MAP Float) RETURNS (folder: Folder) ERRORS [| |],",
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
            "MAP Folder",
            "MAP Shade",
            "MAP: LIST Text",
            "MAP Shade"
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

#[test]
fn the_request_decoder_is_what_its_grammar_generates() {
    let grammar = read("request.grammar.anyx");
    let output = anyx_generator::generate(&grammar).unwrap();
    assert!(output.warnings.is_empty(), "{:?}", output.warnings);
    let code = anyx_generator::format(&output.code).expect("rustfmt is installed");
    assert_snapshot("src/request_grammar.rs", &code);
}

#[test]
fn simple_type_names_use_title_case() {
    schema(
        "PROTOCOL a VERSION 1, TYPE File Summary { path: Text }, TYPE ALIAS File Format [| `pdf |], TYPE ALIAS Some Format: File Format,",
    );
    for name in ["file summary", "FileSummary", "File summary"] {
        for declaration in [
            format!("TYPE {name} {{ path: Text }}"),
            format!("TYPE ALIAS {name} [| `pdf |]"),
            format!("TYPE {name}: Text"),
        ] {
            let message = diagnostics(&format!("PROTOCOL a VERSION 1, {declaration},"));
            assert!(
                message.contains("expected words in Title Case"),
                "{message}"
            );
        }
    }
}
