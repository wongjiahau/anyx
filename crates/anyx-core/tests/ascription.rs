//! Ascription: `(red : Colour)` is `red`, checked against `Colour`.

use anyx_core::{
    kernel::{Document, TypeChecker},
    lowering::lower_document,
    parser::parse_document,
};

fn check(source: &str) -> Result<(), String> {
    let parsed = parse_document(source);
    assert!(
        parsed.syntax_errors.is_empty(),
        "{:?}",
        parsed.syntax_errors
    );
    let lowered = lower_document(&parsed.document.map_err(|error| error.render(source, "x"))?)
        .map_err(|error| error.message)?;
    TypeChecker::default()
        .check_document(&Document::try_from(&lowered).unwrap())
        .map(|_| ())
        .map_err(|error| error.to_string())
}

const COLOURS: &str = "
    DATA Colour WITH [red, green],
    DATA Light WITH [amber],
    FUNCTION paint (colour: Colour) : Colour IS colour,";

#[test]
fn an_ascription_has_the_type_it_names() {
    assert_eq!(
        check(&format!(
            "{COLOURS} FUNCTION c : Colour IS (red : Colour),
             FUNCTION p : Colour IS paint (green : Colour),"
        )),
        Ok(())
    );
}

#[test]
fn an_ascription_must_fit_what_it_ascribes() {
    assert!(check(&format!("{COLOURS} FUNCTION c : Light IS (red : Light),")).is_err());
    assert!(check(&format!("{COLOURS} FUNCTION c : Light IS (red : Colour),")).is_err());
}

#[test]
fn what_an_ascription_names_must_be_a_type() {
    assert!(check(&format!("{COLOURS} FUNCTION c : Colour IS (red : green),")).is_err());
}

#[test]
fn a_parameter_before_an_arrow_is_still_a_function_type() {
    assert_eq!(
        check(
            "FUNCTION (m: Natural) plus (n: Natural) : Natural IS m,
             FUNCTION add : ((m: Natural) -> ⁽(n: Natural) -> Natural) IS (_ plus _),"
        ),
        Ok(())
    );
}
