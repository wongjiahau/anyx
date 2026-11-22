//! Explicit imports, visibility and a flat wire namespace.
mod common;

use anyx_protocol::Schema;
use common::{assert_snapshot, scoped};

fn load(entry: &str, files: &[(&str, &str)]) -> Result<Schema, Vec<anyx_protocol::Diagnostic>> {
    Schema::load(entry, |path| {
        files
            .iter()
            .find(|(file, _)| *file == path)
            .map(|(_, source)| source.to_string())
            .ok_or("file not found")
    })
}

#[test]
fn methods_and_types_have_flat_names() {
    let schema = scoped();
    let names: Vec<_> = schema
        .methods
        .iter()
        .map(|method| method.full_name())
        .collect();
    assert_eq!(
        names,
        [
            "list",
            "read _",
            "write _ at revision _ content _",
            "trash _",
            "empty trash",
            "html _",
            "pdf _"
        ]
    );
    let write = schema.method("write _ at revision _ content _").unwrap();
    assert_eq!(
        schema.type_name(&write.returns.as_ref().unwrap().r#type),
        "LIST Diagnostic"
    );
    let errors: Vec<_> = schema
        .errors_of(schema.method("pdf _").unwrap())
        .iter()
        .map(|error| error.shape.text.as_str())
        .collect();
    assert_eq!(errors, ["unreadable _", "trashed", "missing"]);
    let calls = anyx_protocol::decode_request(&schema, "pdf \"a.note\", list").unwrap();
    assert_eq!(
        anyx_protocol::encode_request(&schema, &calls).unwrap(),
        "pdf \"a.note\",\nlist,\n"
    );
    assert!(anyx_protocol::decode_request(&schema, "files/export/pdf \"a.note\"").is_err());
}

#[cfg(feature = "json")]
#[test]
fn json_uses_flat_names() {
    let schema = scoped();
    assert_eq!(
        anyx_protocol::json::encode_request_json(&schema, "pdf _", "[\"a\"]").unwrap(),
        "pdf \"a\"\n"
    );
    assert_eq!(
        anyx_protocol::json::decode_response_json(&schema, "pdf _", "ERROR `missing").unwrap(),
        r#"{"error":{"kind":"missing"},"kind":"error"}"#
    );
}

#[test]
fn guide_explains_imports_and_flat_names() {
    assert_snapshot(
        "tests/snapshots/scoped_guide.md",
        &anyx_protocol::guide(&scoped(), "Send a request as the body of `POST /notes`."),
    );
}

#[test]
fn relative_paths_diamonds_and_unused_files() {
    let files = [
        (
            "notes/Entry File.anyx",
            "PROTOCOL notes VERSION 1, USE \"./a.anyx\", USE \"./sub/b.anyx\", METHOD root RETURNS (x: Text) ERRORS [| |],",
        ),
        (
            "notes/a.anyx",
            "USE \"../shared.anyx\", TYPE ALIAS First: { item: Shared },",
        ),
        (
            "notes/sub/b.anyx",
            "USE \"../../shared.anyx\", TYPE ALIAS Second: { item: Shared },",
        ),
        ("shared.anyx", "PUBLIC TYPE ALIAS Shared: { value: Text },"),
        ("notes/draft.anyx", "this is not a valid schema"),
    ];
    let mut reads = std::collections::BTreeMap::new();
    let schema = Schema::load("notes/./Entry File.anyx", |path| {
        *reads.entry(path.to_string()).or_insert(0) += 1;
        files
            .iter()
            .find(|(file, _)| *file == path)
            .map(|(_, source)| source.to_string())
            .ok_or("missing")
    })
    .unwrap();
    assert_eq!(schema.files.len(), 4);
    assert_eq!(reads["shared.anyx"], 1);
    assert!(!reads.contains_key("notes/draft.anyx"));
    assert_eq!(schema.methods[0].full_name(), "root");
}

#[test]
fn shared_library_can_be_checked_or_used_by_several_protocols() {
    let files = [
        (
            "notes.anyx",
            "PROTOCOL notes VERSION 1, USE \"./shared.anyx\",",
        ),
        (
            "billing.anyx",
            "PROTOCOL billing VERSION 1, USE \"./shared.anyx\",",
        ),
        (
            "shared.anyx",
            "PUBLIC TYPE ALIAS Session: { user: Text }, METHOD current RETURNS (session: Session) ERRORS [| |],",
        ),
    ];
    for entry in ["notes.anyx", "billing.anyx", "shared.anyx"] {
        let schema = load(entry, &files).unwrap();
        assert_eq!(schema.types.len(), 3);
        assert_eq!(schema.methods[0].full_name(), "current");
        assert_eq!(schema.is_protocol(), entry != "shared.anyx");
    }
}

#[test]
fn own_declarations_and_direct_public_imports_are_visible() {
    let schema = load("entry.protocol.anyx", &[
        ("entry.protocol.anyx", "PROTOCOL p VERSION 1, USE \"./lib.anyx\", TYPE ALIAS Local: { value: Item }, METHOD f RETURNS (x: Local) ERRORS [| Failures |],"),
        ("lib.anyx", "PUBLIC TYPE ALIAS Item: { value: Text }, PUBLIC TYPE ALIAS Failures [| `gone |],"),
    ]).unwrap();
    // `Failures`, an alias of a union of variants, is a type of its own.
    assert_eq!(schema.types.len(), 5);
    assert_eq!(schema.methods[0].errors.len(), 1);
}

#[test]
fn imports_do_not_reexport_and_private_names_stay_private() {
    for declaration in [
        "PUBLIC TYPE ALIAS Item: { value: Text },",
        "TYPE ALIAS Item: { value: Text },",
    ] {
        let diagnostics = load(
            "entry.protocol.anyx",
            &[
                (
                    "entry.protocol.anyx",
                    "PROTOCOL p VERSION 1, USE \"./middle.anyx\", TYPE ALIAS Local: { value: Item },",
                ),
                ("middle.anyx", "USE \"./lib.anyx\","),
                ("lib.anyx", declaration),
            ],
        )
        .unwrap_err();
        assert!(
            diagnostics
                .iter()
                .any(|error| error.message.contains("not visible"))
        );
    }
    let errors = load(
        "entry.protocol.anyx",
        &[
            (
                "entry.protocol.anyx",
                "USE \"./lib.anyx\", METHOD f RETURNS (x: Text) ERRORS [| Failures |],",
            ),
            ("lib.anyx", "TYPE ALIAS Failures [| `gone |],"),
        ],
    )
    .unwrap_err();
    assert!(
        errors
            .iter()
            .any(|error| error.message.contains("not visible"))
    );
}

#[test]
fn duplicate_names_include_both_files_even_when_private() {
    for second in [
        "TYPE ALIAS Item: { value: Text },",
        "TYPE ALIAS Item [| `gone |],",
    ] {
        let diagnostics = load(
            "entry.protocol.anyx",
            &[
                ("entry.protocol.anyx", "USE \"./a.anyx\", USE \"./b.anyx\","),
                ("a.anyx", "TYPE ALIAS Item: { value: Text },"),
                ("b.anyx", second),
            ],
        )
        .unwrap_err();
        assert!(
            diagnostics
                .iter()
                .any(|error| error.message.contains("a.anyx") && error.message.contains("b.anyx")),
            "{diagnostics:?}"
        );
    }
}

#[test]
fn duplicate_methods_across_files_are_rejected() {
    assert!(
        load(
            "entry.protocol.anyx",
            &[
                ("entry.protocol.anyx", "USE \"./a.anyx\", USE \"./b.anyx\","),
                ("a.anyx", "METHOD f RETURNS (x: Text) ERRORS [| |],"),
                ("b.anyx", "METHOD f RETURNS (x: Text) ERRORS [| |],"),
            ]
        )
        .unwrap_err()
        .iter()
        .any(|error| error.message.contains("found overlapping methods"))
    );
}

#[test]
fn cycles_point_at_the_closing_use_and_show_the_cycle() {
    let errors = load(
        "a.anyx",
        &[
            ("a.anyx", "USE \"./sub/b.anyx\","),
            ("sub/b.anyx", "USE \"../c.anyx\","),
            ("c.anyx", "USE \"./a.anyx\","),
        ],
    )
    .unwrap_err();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].path, "c.anyx");
    assert_eq!(errors[0].span.start, 0);
    assert!(
        errors[0]
            .message
            .contains("a.anyx → sub/b.anyx → c.anyx → a.anyx")
    );
    assert!(
        load("a.anyx", &[("a.anyx", "USE \"./a.anyx\",")]).unwrap_err()[0]
            .message
            .contains("a.anyx → a.anyx")
    );
}

#[test]
fn invalid_imports_are_reported_at_the_use() {
    for path in [
        "/common.protocol.anyx",
        "",
        "accounts:session.anyx",
        "./missing.anyx",
    ] {
        let source = format!("PROTOCOL p VERSION 1, USE {path:?},");
        let errors = load("entry.protocol.anyx", &[("entry.protocol.anyx", &source)]).unwrap_err();
        assert_eq!(errors[0].path, "entry.protocol.anyx");
        assert_eq!(errors[0].span.start, 22);
    }
    let errors = load(
        "entry.protocol.anyx",
        &[
            ("entry.protocol.anyx", "USE \"./other.anyx\","),
            ("other.anyx", "PROTOCOL other VERSION 1,"),
        ],
    )
    .unwrap_err();
    assert!(errors[0].message.contains("`PROTOCOL` header"));
    assert_eq!(errors[0].path, "entry.protocol.anyx");
}

#[test]
fn recursive_types_remain_valid_inside_a_library() {
    Schema::parse(
        "TYPE ALIAS Folder: { entries: LIST Entry }, TYPE ALIAS Entry: { parent: Folder },",
    )
    .unwrap();
}

#[test]
fn overloads_across_files_use_plain_annotations_and_json_names() {
    let schema = load(
        "notes.anyx",
        &[
            (
                "notes.anyx",
                "PROTOCOL notes VERSION 1, USE \"./files.protocol.anyx\", USE \"./shares.anyx\",",
            ),
            (
                "files.protocol.anyx",
                "NAMED TYPE File Id (value: Text), METHOD open (id: File Id) RETURNS (text: Text) ERRORS [| |],",
            ),
            (
                "shares.anyx",
                "NAMED TYPE Share Id (value: Text), METHOD open (id: Share Id) RETURNS (text: Text) ERRORS [| |],",
            ),
        ],
    )
    .unwrap();
    let file = anyx_protocol::decode_request(&schema, "open (File Id \"abc\")").unwrap();
    let share = anyx_protocol::decode_request(&schema, "open (Share Id \"abc\")").unwrap();
    assert_ne!(file[0].method, share[0].method);
    assert_eq!(
        schema.call_name(schema.method_at(file[0].method)),
        "open (File Id)"
    );
    assert_eq!(
        anyx_protocol::encode_request(&schema, &file).unwrap(),
        "open (File Id \"abc\")\n"
    );
    assert!(anyx_protocol::decode_request(&schema, "open (\"abc\" AS files/File Id)").is_err());
    #[cfg(feature = "json")]
    assert_eq!(
        anyx_protocol::json::encode_request_json(
            &schema,
            "open (File Id)",
            r#"[{"FileId": "abc"}]"#
        )
        .unwrap(),
        "open (File Id \"abc\")\n"
    );
}

#[test]
fn moving_an_imported_file_does_not_change_wire_names() {
    let declaration = "PUBLIC NAMED TYPE File Id (value: Text), METHOD open (id: File Id) RETURNS (text: Text) ERRORS [| |],";
    let before = load(
        "entry.protocol.anyx",
        &[
            (
                "entry.protocol.anyx",
                "PROTOCOL p VERSION 1, USE \"./files.protocol.anyx\",",
            ),
            ("files.protocol.anyx", declaration),
        ],
    )
    .unwrap();
    let after = load(
        "entry.protocol.anyx",
        &[
            (
                "entry.protocol.anyx",
                "PROTOCOL p VERSION 1, USE \"./Draft Folder/renamed.anyx\",",
            ),
            ("Draft Folder/renamed.anyx", declaration),
        ],
    )
    .unwrap();
    let request = "open (File Id \"abc\")";
    assert_eq!(
        anyx_protocol::decode_request(&before, request).unwrap(),
        anyx_protocol::decode_request(&after, request).unwrap()
    );
    assert_eq!(
        before.call_name(&before.methods[0]),
        after.call_name(&after.methods[0])
    );
}

#[test]
fn absolute_entry_at_filesystem_root_keeps_relative_imports_absolute() {
    let schema = load(
        "/entry.protocol.anyx",
        &[
            (
                "/entry.protocol.anyx",
                "USE \"./shared.anyx\", USE \"./sub/../shared.anyx\",",
            ),
            ("/shared.anyx", "TYPE ALIAS Shared: { value: Text },"),
        ],
    )
    .unwrap();
    assert_eq!(schema.files.len(), 2);
    assert_eq!(schema.files[1].path, "/shared.anyx");
}
