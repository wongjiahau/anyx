//! `RECORD`: one constructor, written as `{ }` with its fields in any order.

use anyx_core::{
    kernel::{Declaration, Document, TypeChecker},
    lowering::lower_document,
    parser::parse_document,
};

const DATA: &str = "
    DATA Case RETURNS Type WITH [smart case RETURNS Case, any case RETURNS Case],
    DATA Hidden files RETURNS Type WITH [hidden RETURNS Hidden files, skip hidden RETURNS Hidden files],
    DATA Vector of (length: Natural) RETURNS Type WITH [
        empty RETURNS Vector of (zero),
        prepend length (length: Natural) value (first: Natural) tail (rest: Vector of (length))
            RETURNS Vector of (successor of (length)),
    ],
    RECORD Search RETURNS Type WITH [
        (case: Case) DEFAULT smart case,
        (hidden: Hidden files) DEFAULT skip hidden,
        between (low: Natural) and (high: Natural) DEFAULT between (zero) and (zero),
        max depth (max depth: Natural),
    ],
    RECORD Sized list RETURNS Type WITH [
        length (length: Natural),
        items (items: Vector of (length)),
    ],";

fn lowered(source: &str) -> Result<Document, String> {
    let parsed = parse_document(&format!("{DATA}\n{source}"));
    assert!(
        parsed.syntax_errors.is_empty(),
        "{:?}",
        parsed.syntax_errors
    );
    let lowered = lower_document(&parsed.document.unwrap()).map_err(|error| error.message)?;
    Ok(Document::try_from(&lowered).unwrap())
}

fn check(source: &str) -> Result<(), String> {
    let document = lowered(source)?;
    TypeChecker::default()
        .check_document(&document)
        .map(|_| ())
        .map_err(|error| error.to_string())
}

/// The normal form of each function's body, by name.
fn bodies(source: &str) -> Vec<(String, anyx_core::kernel::Term)> {
    let document = lowered(source).unwrap();
    let checker = TypeChecker::default().check_document(&document).unwrap();
    document
        .declarations
        .iter()
        .filter_map(|declaration| match declaration {
            Declaration::Function { signature, body } => {
                Some((signature.name.clone(), checker.normalize(body).unwrap()))
            }
            _ => None,
        })
        .collect()
}

#[test]
fn a_value_fills_its_fields_in_declaration_order_and_defaults_the_rest() {
    let bodies = bodies(
        "FUNCTION written RETURNS Search IS { max depth (zero), any case },
         FUNCTION spelled out RETURNS Search
           IS (search (any case) (skip hidden) (zero) (zero) (zero)),",
    );
    let body = |name: &str| &bodies.iter().find(|(found, _)| found == name).unwrap().1;
    assert_eq!(body("written"), body("spelled out"));
}

#[test]
fn a_field_s_type_can_name_an_earlier_field() {
    assert_eq!(
        check(
            "FUNCTION one item RETURNS Sized list IS {
               items (prepend length (zero) value (zero) tail (empty)),
               length (successor of (zero)),
             },"
        ),
        Ok(())
    );
    assert!(
        check(
            "FUNCTION wrong RETURNS Sized list IS { items (empty), length (successor of (zero)) },"
        )
        .unwrap_err()
        .contains("expected type")
    );
}

#[test]
fn a_value_must_give_every_field_without_a_default() {
    assert_eq!(
        check("FUNCTION wrong RETURNS Search IS { any case },"),
        Err("no record has exactly these fields".to_owned())
    );
}

#[test]
fn a_default_is_checked_against_its_field() {
    assert!(
        check("RECORD Pick RETURNS Type WITH [(case: Case) DEFAULT hidden],")
            .unwrap_err()
            .contains("default case of Pick")
    );
}

#[test]
fn a_field_that_depends_on_another_has_no_default() {
    assert_eq!(
        check(
            "RECORD Pair RETURNS Type WITH [
               length (size: Natural),
               items (elements: Vector of (size)) DEFAULT items (empty),
             ],"
        ),
        Err("`elements` depends on the field `size`, so it has no `DEFAULT`".to_owned())
    );
}

#[test]
fn an_entry_finds_one_field_only() {
    assert_eq!(
        check("RECORD Pair RETURNS Type WITH [(first: Case), (second: Case)],"),
        Err("two fields written as their value alone both hold `Case`, so an entry couldn't tell which it fills".to_owned())
    );
    assert_eq!(
        check("RECORD Pair RETURNS Type WITH [successor of (x: Natural), (n: Natural)],"),
        Err("`successor of _` already names another field or a value of one, so an entry couldn't tell which it fills".to_owned())
    );
    assert_eq!(
        check("RECORD Pair RETURNS Type WITH [successor of (x: Natural)],"),
        Ok(())
    );
    assert_eq!(
        check("RECORD Pair RETURNS Type WITH [first (x: Natural), second (x: Natural)],"),
        Err("duplicate binding of `x`".to_owned())
    );
}

#[test]
fn a_record_s_parameters_come_first_in_its_constructor() {
    assert_eq!(
        check(
            "RECORD Box of (item: Type) RETURNS Kind WITH [content (content: item)],
             FUNCTION boxed RETURNS Box of (Natural) IS (box of (Natural) (zero)),"
        ),
        Ok(())
    );
}

#[test]
fn a_record_can_be_an_unnamed_field() {
    let source = "RECORD Span RETURNS Type WITH [from (low: Natural) to (high: Natural)],
         RECORD Labelled RETURNS Type WITH [(span: Span), labelled (label: Case)],
         FUNCTION nested RETURNS Labelled IS { labelled (any case), { from 1 to 2 } },
         FUNCTION explicit RETURNS Labelled
            IS labelled (span (1) (2)) (any case),";
    check(source).unwrap();
    let bodies = bodies(source);
    assert_eq!(bodies[bodies.len() - 2].1, bodies[bodies.len() - 1].1);
}
