//! `:` left out: a `DATA` or `RECORD` without it is a `Type`, and a
//! constructor without it returns the type it is a constructor of.

use anyx_core::{
    kernel::{Declaration, Document, TypeChecker},
    lowering::lower_document,
    parser::parse_document,
};

fn lowered(source: &str) -> Result<Document, String> {
    let parsed = parse_document(source);
    assert!(
        parsed.syntax_errors.is_empty(),
        "{:?}",
        parsed.syntax_errors
    );
    let lowered = lower_document(&parsed.document.unwrap()).map_err(|error| error.message)?;
    Ok(Document::try_from(&lowered).unwrap())
}

fn check(source: &str) -> Result<(), String> {
    TypeChecker::default()
        .check_document(&lowered(source)?)
        .map(|_| ())
        .map_err(|error| error.to_string())
}

#[test]
fn a_shorthand_declares_what_the_full_spelling_does() {
    let short = "
        DATA Shape WITH [
          circle (radius: Natural),
          square,
        ],
        RECORD Version WITH [revision (revision: Natural)],";
    let full = "
        DATA Shape : Type WITH [
          circle (radius: Natural) : Shape,
          square : Shape,
        ],
        RECORD Version : Type WITH [revision (revision: Natural)],";
    check(short).unwrap();
    let declarations = |source| -> Vec<Declaration> { lowered(source).unwrap().declarations };
    assert_eq!(declarations(short), declarations(full));
}

#[test]
fn constructors_with_and_without_returns_mix() {
    check(
        "DATA Shape WITH [circle, square : Shape],
         FUNCTION corners of (shape: Shape) : Natural IS (MATCH shape WITH [
           WHEN circle THEN 0,
           WHEN square THEN 4,
         ]),",
    )
    .unwrap();
}

#[test]
fn a_type_with_parameters_says_what_its_constructors_return() {
    let error = check("DATA Box of (item: Type) WITH [empty],").unwrap_err();
    assert!(
        error.contains(
            "`Box of _` takes parameters, so each of its constructors says what it returns with `:`"
        ),
        "{error}"
    );
    check(
        "DATA Box of (item: Type) WITH [
           empty (item: Type) : Box of (item),
         ],",
    )
    .unwrap();
}

#[test]
fn a_kind_is_still_written_out() {
    check(
        "DATA Effect WITH [reads, writes],
         DATA Endpoint (effect: Effect) : Kind WITH [
           ping : Endpoint (reads),
         ],",
    )
    .unwrap();
}
