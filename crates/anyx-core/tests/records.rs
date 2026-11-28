//! `RECORD`: one constructor, written as `{ }` with its fields in any order.

use anyx_core::{
    kernel::{Declaration, Document, TypeChecker},
    lowering::lower_document,
    parser::parse_document,
};

const DATA: &str = "
    DATA Case : Type WITH [smart case : Case, any case : Case],
    DATA Hidden files : Type WITH [hidden : Hidden files, skip hidden : Hidden files],
    DATA Vector of (length: Natural) : Type WITH [
        empty : Vector of (zero),
        prepend length (length: Natural) value (first: Natural) tail (rest: Vector of (length))
            : Vector of (successor of (length)),
    ],
    RECORD Search : Type WITH [
        .(case: Case) DEFAULT smart case,
        .(hidden: Hidden files) DEFAULT skip hidden,
        between (low: Natural) and (high: Natural) DEFAULT between (zero) and (zero),
        max depth (max depth: Natural),
    ],
    RECORD Sized list : Type WITH [
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
        "FUNCTION written : Search IS { max depth (zero), .any case },
         FUNCTION spelled out : Search
           IS (search (any case) (skip hidden) (zero) (zero) (zero)),",
    );
    let body = |name: &str| &bodies.iter().find(|(found, _)| found == name).unwrap().1;
    assert_eq!(body("written"), body("spelled out"));
}

#[test]
fn a_field_s_type_can_name_an_earlier_field() {
    assert_eq!(
        check(
            "FUNCTION one item : Sized list IS {
               items (prepend length (zero) value (zero) tail (empty)),
               length (successor of (zero)),
             },"
        ),
        Ok(())
    );
    assert!(
        check("FUNCTION wrong : Sized list IS { items (empty), length (successor of (zero)) },")
            .unwrap_err()
            .contains("expected type")
    );
}

#[test]
fn a_value_must_give_every_field_without_a_default() {
    assert_eq!(
        check("FUNCTION wrong : Search IS { .any case },"),
        Err("no record has exactly these fields".to_owned())
    );
}

#[test]
fn a_default_is_checked_against_its_field() {
    assert!(
        check("RECORD Pick : Type WITH [.(case: Case) DEFAULT hidden],")
            .unwrap_err()
            .contains("default case of Pick")
    );
}

#[test]
fn a_field_that_depends_on_another_has_no_default() {
    assert_eq!(
        check(
            "RECORD Pair : Type WITH [
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
        check("RECORD Pair : Type WITH [.(first: Case), .(second: Case)],"),
        Err(
            "two fields marked `.` both hold `Case`, so an entry couldn't tell which it fills"
                .to_owned()
        )
    );
    // `.` tells a value from a field's words, so they may be the same.
    assert_eq!(
        check(
            "DATA Step WITH [next of (n: Natural)],
             RECORD Pair WITH [next of (x: Natural), .(step: Step)],
             FUNCTION both : Pair IS { next of 1, .next of 2 },"
        ),
        Ok(())
    );
    assert_eq!(
        check("RECORD Pair : Type WITH [successor of (x: Natural)],"),
        Ok(())
    );
    assert_eq!(
        check("RECORD Pair : Type WITH [first (x: Natural), second (x: Natural)],"),
        Err("duplicate binding of `x`".to_owned())
    );
}

#[test]
fn a_record_s_parameters_come_first_in_its_constructor() {
    assert_eq!(
        check(
            "RECORD Box of (item: Type) : Kind WITH [content (content: item)],
             FUNCTION boxed : Box of (Natural) IS (box of (Natural) (zero)),"
        ),
        Ok(())
    );
}

#[test]
fn a_record_can_be_an_unnamed_field() {
    let source = "RECORD Span : Type WITH [from (low: Natural) to (high: Natural)],
         RECORD Labelled : Type WITH [.(span: Span), labelled (label: Case)],
         FUNCTION nested : Labelled IS { labelled (any case), .{ from 1 to 2 } },
         FUNCTION explicit : Labelled
            IS labelled (span (1) (2)) (any case),";
    check(source).unwrap();
    let bodies = bodies(source);
    assert_eq!(bodies[bodies.len() - 2].1, bodies[bodies.len() - 1].1);
}

#[test]
fn a_value_of_a_dot_field_is_marked_with_a_dot() {
    assert_eq!(
        check("FUNCTION wrong : Search IS { max depth (zero), any case },"),
        Err("no record has exactly these fields".to_owned())
    );
    assert_eq!(
        check("FUNCTION wrong : Search IS { max depth (zero), .max depth (zero) },"),
        Err("`.` marks a value of a `.` field, but `max depth _` is no constructor".to_owned())
    );
}
