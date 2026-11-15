//! The code generated from Poentaro's schema, from the schema of scopes in
//! `anyx-protocol`'s fixtures, and from `tests/fixtures/types.anyx`,
//! `tests/fixtures/named.anyx`, `tests/fixtures/generic.anyx` and
//! `tests/fixtures/overloaded.anyx`, kept in `tests/generated` so a change
//! to the generator shows up as a diff. `round_trip.rs`,
//! `scoped_round_trip.rs`, `types_round_trip.rs`, `named_round_trip.rs`,
//! `generic_round_trip.rs` and `overloaded_round_trip.rs` compile the Rust
//! modules.

use anyx_protocol::{Schema, SchemaFile};

fn read(path: &str) -> String {
    let full = format!("{}/{path}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(&full).unwrap_or_else(|error| panic!("{full}: {error}"))
}

fn assert_snapshot(path: &str, actual: &str) {
    if std::env::var_os("UPDATE_SNAPSHOTS").is_some() {
        let full = format!("{}/{path}", env!("CARGO_MANIFEST_DIR"));
        std::fs::write(&full, actual).unwrap_or_else(|error| panic!("{full}: {error}"));
    } else {
        assert!(
            read(path) == actual,
            "{path} is out of date; run with UPDATE_SNAPSHOTS=1 and review the diff"
        );
    }
}

fn poentaro() -> Schema {
    Schema::parse(&read(
        "../anyx-protocol/tests/fixtures/poentaro.protocol.anyx",
    ))
    .unwrap()
}

/// The scoped fixture: a file per scope.
fn scoped() -> Schema {
    let paths = [
        "protocol.anyx",
        "common.anyx",
        "files.anyx",
        "files/export.anyx",
    ];
    let sources: Vec<String> = paths
        .iter()
        .map(|path| read(&format!("../anyx-protocol/tests/fixtures/scoped/{path}")))
        .collect();
    let files: Vec<SchemaFile> = paths
        .iter()
        .zip(&sources)
        .map(|(path, source)| SchemaFile { path, source })
        .collect();
    Schema::parse_files(&files).unwrap()
}

/// Every kind of type.
fn types() -> Schema {
    Schema::parse(&read("tests/fixtures/types.anyx")).unwrap()
}

#[test]
fn the_rust_module_of_every_type_is_up_to_date() {
    let code = anyx_protocol_generate::rust(&types()).unwrap();
    let code = anyx_protocol_generate::format_rust(&code).expect("rustfmt is installed");
    assert_snapshot("tests/generated/types.rs", &code);
}

#[test]
fn the_typescript_client_of_every_type_is_up_to_date() {
    let code = anyx_protocol_generate::typescript(&types()).unwrap();
    assert_snapshot("tests/generated/types.ts", &code);
}

/// Named types, each over a type of another kind.
fn named() -> Schema {
    Schema::parse(&read("tests/fixtures/named.anyx")).unwrap()
}

#[test]
fn the_rust_module_of_named_types_is_up_to_date() {
    let code = anyx_protocol_generate::rust(&named()).unwrap();
    let code = anyx_protocol_generate::format_rust(&code).expect("rustfmt is installed");
    assert_snapshot("tests/generated/named.rs", &code);
}

#[test]
fn the_typescript_client_of_named_types_is_up_to_date() {
    let code = anyx_protocol_generate::typescript(&named()).unwrap();
    assert_snapshot("tests/generated/named.ts", &code);
}

#[test]
fn the_scoped_rust_module_is_up_to_date() {
    let code = anyx_protocol_generate::rust(&scoped()).unwrap();
    let code = anyx_protocol_generate::format_rust(&code).expect("rustfmt is installed");
    assert_snapshot("tests/generated/scoped.rs", &code);
}

#[test]
fn the_scoped_typescript_client_is_up_to_date() {
    let code = anyx_protocol_generate::typescript(&scoped()).unwrap();
    assert_snapshot("tests/generated/scoped.ts", &code);
}

#[test]
fn the_rust_module_is_up_to_date() {
    let code = anyx_protocol_generate::rust(&poentaro()).unwrap();
    let code = anyx_protocol_generate::format_rust(&code).expect("rustfmt is installed");
    assert_snapshot("tests/generated/poentaro.rs", &code);
}

#[test]
fn the_wasm_bindings_are_up_to_date() {
    let code = anyx_protocol_generate::wasm(&poentaro());
    let code = anyx_protocol_generate::format_rust(&code).expect("rustfmt is installed");
    assert_snapshot("tests/generated/poentaro_wasm.rs", &code);
}

#[test]
fn the_typescript_client_is_up_to_date() {
    let code = anyx_protocol_generate::typescript(&poentaro()).unwrap();
    assert_snapshot("tests/generated/poentaro.ts", &code);
}

fn error(source: &str) -> String {
    let schema = Schema::parse(source).unwrap();
    let rust = anyx_protocol_generate::rust(&schema).unwrap_err();
    let typescript = anyx_protocol_generate::typescript(&schema).unwrap_err();
    assert_eq!(rust, typescript);
    rust.message
}

#[test]
fn names_that_become_the_same_name_are_rejected() {
    assert_eq!(
        error(
            "PROTOCOL p VERSION 1, RECORD read error { a: TEXT }, METHOD read (path: TEXT) RETURNS (text: TEXT) ERRORS [gone]"
        ),
        "found the errors of `read _`, which becomes the type `ReadError`, like the type `read error`; expected names that stay distinct in the generated code, so rename one"
    );
    assert_eq!(
        error("PROTOCOL p VERSION 1, RECORD client { a: TEXT }"),
        "found the type `client`, which becomes the type `Client`, like the generated `Client`; expected names that stay distinct in the generated code, so rename one"
    );
    assert_eq!(
        error("PROTOCOL p VERSION 1, CHOICE record { a }"),
        "found the type `record`, which becomes the type `Record`, like the generated `Record`; expected names that stay distinct in the generated code, so rename one"
    );
    assert_eq!(
        error("PROTOCOL p VERSION 1, METHOD send (text: TEXT) RETURNS (sent: BOOLEAN)"),
        "found `send _`, which becomes the method `send`, like the client's `send`; expected names that stay distinct in the generated code, so rename one"
    );
}

#[test]
fn generic_types_whose_words_are_the_same_are_rejected() {
    assert_eq!(
        error(
            "PROTOCOL p VERSION 1, RECORD page of (item: TYPE) { items: LIST: item }, RECORD page (item: TYPE) of { item: item }"
        ),
        "found the type `page (item) of`, which becomes the type `PageOf`, like the type `page of (item)`; expected names that stay distinct in the generated code, so rename one"
    );
    assert_eq!(
        error(
            "PROTOCOL p VERSION 1, RECORD page { a: TEXT }, RECORD page (item: TYPE) { item: item }"
        ),
        "found the type `page (item)`, which becomes the type `Page`, like the type `page`; expected names that stay distinct in the generated code, so rename one"
    );
}

#[test]
fn a_keyword_becomes_a_raw_identifier() {
    let schema =
        Schema::parse("PROTOCOL p VERSION 1, RECORD box { type: TEXT, loop: INTEGER }").unwrap();
    let code = anyx_protocol_generate::rust(&schema).unwrap();
    assert!(code.contains("pub r#type: String"), "{code}");
    assert!(code.contains("pub r#loop: i64"), "{code}");
}

fn scoped_error(files: &[(&str, &str)]) -> String {
    let files: Vec<SchemaFile> = files
        .iter()
        .map(|(path, source)| SchemaFile { path, source })
        .collect();
    let schema = Schema::parse_files(&files).unwrap();
    let rust = anyx_protocol_generate::rust(&schema).unwrap_err();
    let typescript = anyx_protocol_generate::typescript(&schema).unwrap_err();
    assert_eq!(rust, typescript);
    rust.message
}

#[test]
fn names_in_scopes_that_become_the_same_name_are_rejected() {
    let root = ("protocol.anyx", "PROTOCOL p VERSION 1,");
    assert_eq!(
        scoped_error(&[
            root,
            ("files.anyx", "METHOD read RETURNS (a: TEXT),"),
            ("files/read.anyx", "METHOD open RETURNS (a: TEXT),"),
        ]),
        "found the scope `files/read` in the scope `files`, which becomes the method `read`, like `files/read`; expected names that stay distinct in the generated code, so rename one"
    );
    assert_eq!(
        scoped_error(&[
            root,
            ("files.anyx", "RECORD export page { a: TEXT },"),
            ("files/export.anyx", "RECORD page { a: TEXT },"),
        ]),
        "found the type `files/export/page`, which becomes the TypeScript type `FilesExportPage`, like the type `files/export page`; expected names that stay distinct in the generated code, so rename one"
    );
}

/// Generic records and choices, and their uses.
fn generic() -> Schema {
    Schema::parse(&read("tests/fixtures/generic.anyx")).unwrap()
}

#[test]
fn the_rust_module_of_generic_types_is_up_to_date() {
    let code = anyx_protocol_generate::rust(&generic()).unwrap();
    let code = anyx_protocol_generate::format_rust(&code).expect("rustfmt is installed");
    assert_snapshot("tests/generated/generic.rs", &code);
}

#[test]
fn the_typescript_client_of_generic_types_is_up_to_date() {
    let code = anyx_protocol_generate::typescript(&generic()).unwrap();
    assert_snapshot("tests/generated/generic.ts", &code);
}

/// Methods of one shape with different parameter types.
fn overloaded() -> Schema {
    Schema::parse(&read("tests/fixtures/overloaded.anyx")).unwrap()
}

#[test]
fn the_rust_module_of_overloads_is_up_to_date() {
    let code = anyx_protocol_generate::rust(&overloaded()).unwrap();
    let code = anyx_protocol_generate::format_rust(&code).expect("rustfmt is installed");
    assert_snapshot("tests/generated/overloaded.rs", &code);
}

#[test]
fn the_typescript_client_of_overloads_is_up_to_date() {
    let code = anyx_protocol_generate::typescript(&overloaded()).unwrap();
    assert_snapshot("tests/generated/overloaded.ts", &code);
}

#[test]
fn an_overload_whose_name_another_method_has_is_rejected() {
    assert_eq!(
        error(
            "PROTOCOL p VERSION 1, METHOD tag (a: TEXT) RETURNS NOTHING, METHOD tag (a: INTEGER) RETURNS NOTHING, METHOD tag with text RETURNS NOTHING,"
        ),
        "found `tag with text`, which becomes the method `tag_with_text`, like `tag (TEXT)`; expected names that stay distinct in the generated code, so rename one"
    );
}
