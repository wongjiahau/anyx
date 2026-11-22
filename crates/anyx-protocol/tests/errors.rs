//! A method's errors: `ERRORS [| … |]` after what it returns, always written,
//! holding variants and aliases of unions of variants, flattened in order.

mod common;

use common::{diagnostics, schema};

fn errors(source: &str, method: &str) -> Vec<String> {
    let schema = schema(source);
    let method = schema.method(method).unwrap();
    method
        .errors
        .iter()
        .map(|error| error.shape.text.clone())
        .collect()
}

#[test]
fn a_method_that_cant_fail_writes_empty_errors() {
    let found = errors(
        "PROTOCOL p VERSION 1,
        METHOD ping RETURNS NOTHING ERRORS [| |],",
        "ping",
    );
    assert!(found.is_empty());
}

#[test]
fn a_method_without_errors_is_reported() {
    let found = diagnostics(
        "PROTOCOL p VERSION 1,
        METHOD ping RETURNS NOTHING,",
    );
    assert!(found.contains("found a method without `ERRORS`"), "{found}");
}

#[test]
fn an_alias_is_flattened_in_order() {
    let found = errors(
        "PROTOCOL p VERSION 1,
        TYPE ALIAS Common Errors: [| `offline, `busy |],
        TYPE ALIAS File Errors: [| `trashed, Common Errors |],
        METHOD read (path: Text) RETURNS NOTHING ERRORS [| `locked at (path: Text), File Errors, `too large |],",
        "read _",
    );
    assert_eq!(
        found,
        ["locked at _", "trashed", "offline", "busy", "too large"]
    );
}

#[test]
fn an_alias_may_include_one_declared_after_it() {
    let found = errors(
        "PROTOCOL p VERSION 1,
        TYPE ALIAS File Errors: [| `trashed, Common Errors |],
        TYPE ALIAS Common Errors: [| `offline |],
        METHOD read RETURNS NOTHING ERRORS [| File Errors |],",
        "read",
    );
    assert_eq!(found, ["trashed", "offline"]);
}

#[test]
fn a_generic_alias_is_flattened_with_its_arguments() {
    let schema = schema(
        "PROTOCOL p VERSION 1,
        TYPE ALIAS (Item: TYPE) Conflicts: [| `conflict with (current: Item) |],
        METHOD save RETURNS NOTHING ERRORS [| (Integer) Conflicts |],",
    );
    let method = schema.method("save").unwrap();
    assert_eq!(method.errors.len(), 1);
    assert_eq!(
        method.errors[0].shape.parameters[0].r#type,
        anyx_protocol::schema::Type::Integer
    );
}

#[test]
fn an_alias_may_include_a_generic_alias() {
    let schema = schema(
        "PROTOCOL p VERSION 1,
        TYPE ALIAS (Item: TYPE) Conflicts: [| `conflict with (current: Item) |],
        TYPE ALIAS Save Errors: [| `busy, (Integer) Conflicts |],
        METHOD save RETURNS NOTHING ERRORS [| Save Errors |],",
    );
    let method = schema.method("save").unwrap();
    let texts: Vec<&str> = method
        .errors
        .iter()
        .map(|error| error.shape.text.as_str())
        .collect();
    assert_eq!(texts, ["busy", "conflict with _"]);
    assert_eq!(
        method.errors[1].shape.parameters[0].r#type,
        anyx_protocol::schema::Type::Integer
    );
}

#[test]
fn an_error_named_twice_is_reported() {
    let found = diagnostics(
        "PROTOCOL p VERSION 1,
        TYPE ALIAS File Errors: [| `trashed, `missing |],
        METHOD read RETURNS NOTHING ERRORS [| `missing, File Errors |],",
    );
    assert!(
        found.contains("as a value of both `` `missing `` and `File Errors`"),
        "{found}"
    );
}

#[test]
fn an_alias_including_a_variant_twice_is_reported() {
    let found = diagnostics(
        "PROTOCOL p VERSION 1,
        TYPE ALIAS Common Errors: [| `offline |],
        TYPE ALIAS File Errors: [| `offline, Common Errors |],
        METHOD read RETURNS NOTHING ERRORS [| File Errors |],",
    );
    assert!(
        found.contains("found the variant `offline` twice"),
        "{found}"
    );
}

#[test]
fn an_alias_that_includes_itself_is_not_a_union_of_variants() {
    let found = diagnostics(
        "PROTOCOL p VERSION 1,
        TYPE ALIAS Loop Errors: [| `stuck, Loop Errors |],
        METHOD spin RETURNS NOTHING ERRORS [| Loop Errors |],",
    );
    assert!(found.contains("among a method's errors"), "{found}");
}

#[test]
fn an_error_that_is_not_a_variant_is_reported() {
    for member in ["Integer", "[| `a, `b |]", "Maybe Text"] {
        let found = diagnostics(&format!(
            "PROTOCOL p VERSION 1,
            TYPE ALIAS Maybe Text: [| Text, `none |],
            METHOD f RETURNS NOTHING ERRORS [| {member} |],"
        ));
        assert!(
            found.contains("among a method's errors; expected a variant"),
            "{member}: {found}"
        );
    }
}

#[test]
fn an_errors_group_is_reported_with_its_replacement() {
    let found = diagnostics(
        "PROTOCOL p VERSION 1,
        ERRORS file errors [`trashed],
        METHOD f RETURNS NOTHING ERRORS [| |],",
    );
    assert!(found.contains("found an `ERRORS` group"), "{found}");
}
