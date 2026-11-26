//! `3`: a literal of the built-in `Natural`, the same value as
//! `successor of (successor of (successor of (zero)))`.

use anyx_core::{
    kernel::{Declaration, Document, Term, TypeChecker},
    lowering::lower_document,
    parser::parse_document,
};

const DATA: &str = "
    DATA (left: Natural) equals (right: Natural) RETURNS Type WITH [
        reflexivity of (number: Natural) RETURNS ((number) equals (number)),
    ],
    DATA Vector of (length: Natural) RETURNS Type WITH [
        empty RETURNS Vector of (zero),
        prepend length (length: Natural) value (first: Natural) tail (rest: Vector of (length))
            RETURNS Vector of (successor of (length)),
    ],
    FUNCTION (left: Natural) plus (right: Natural) RETURNS Natural
        IS (MATCH left WITH [
            WHEN zero THEN right,
            WHEN successor of (previous) THEN (successor of ((previous) plus (right))),
        ]),";

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

fn body(source: &str, name: &str) -> Term {
    let document = lowered(source).unwrap();
    let checker = TypeChecker::default().check_document(&document).unwrap();
    document
        .declarations
        .iter()
        .find_map(|declaration| match declaration {
            Declaration::Function { signature, body } if signature.name == name => {
                Some(checker.normalize(body).unwrap())
            }
            _ => None,
        })
        .unwrap()
}

#[test]
fn a_literal_is_the_same_value_as_its_constructors() {
    check(
        "FUNCTION three is three RETURNS ((3) equals (successor of (successor of (successor of (zero)))))
            IS (reflexivity of (3)),
         FUNCTION zero is zero RETURNS ((0) equals (zero)) IS (reflexivity of (zero)),",
    )
    .unwrap();
    assert!(check("FUNCTION wrong RETURNS ((3) equals (2)) IS (reflexivity of (3)),").is_err());
}

#[test]
fn arithmetic_on_literals_computes_by_peeling() {
    check("FUNCTION sum RETURNS (((2) plus (3)) equals (5)) IS (reflexivity of (5)),").unwrap();
    assert_eq!(
        body("FUNCTION sum RETURNS Natural IS (2) plus (3),", "sum"),
        Term::Natural(5)
    );
    // A large literal stays one number: only the left side is peeled.
    assert_eq!(
        body(
            "FUNCTION big RETURNS Natural IS (1) plus (10000000000000000000000000),",
            "big"
        ),
        Term::Natural(10000000000000000000000001)
    );
}

#[test]
fn a_match_peels_a_literal() {
    assert_eq!(
        body(
            "FUNCTION predecessor of (number: Natural) RETURNS Natural
                IS (MATCH number WITH [WHEN zero THEN zero, WHEN successor of (previous) THEN previous]),
             FUNCTION two RETURNS Natural IS predecessor of (3),",
            "two"
        ),
        Term::Natural(2)
    );
}

#[test]
fn a_literal_is_an_index() {
    check(
        "FUNCTION pair RETURNS Vector of (2)
            IS (prepend length (1) value (7) tail (prepend length (0) value (8) tail (empty))),
         FUNCTION first of (vector: Vector of (2)) RETURNS Natural
            IS (MATCH vector WITH [WHEN prepend length (length) value (first) tail (rest) THEN first]),",
    )
    .unwrap();
    assert!(
        check(
            "FUNCTION pair RETURNS Vector of (3)
                IS (prepend length (1) value (7) tail (prepend length (0) value (8) tail (empty))),"
        )
        .is_err()
    );
}

#[test]
fn natural_is_built_in() {
    let error = check("DATA Natural RETURNS Type WITH [],").unwrap_err();
    assert!(error.contains("Natural"), "{error}");
    let error = check("FUNCTION huge RETURNS Natural IS 340282366920938463463374607431768211456,")
        .unwrap_err();
    assert!(error.contains("too large"), "{error}");
}
