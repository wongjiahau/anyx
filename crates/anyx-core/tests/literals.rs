//! Literals: `3` of the built-in `Natural` (the same value as `successor
//! of (successor of (successor of (zero)))`) or `Integer`, `"src"` of
//! `Text`, and `[a, b]` of `List of _`.

use anyx_core::{
    kernel::{Declaration, Document, Term, TypeChecker},
    lowering::lower_document,
    parser::parse_document,
};

const DATA: &str = "
    DATA (left: Natural) equals (right: Natural) : Type WITH [
        reflexivity of (number: Natural) : ((number) equals (number)),
    ],
    DATA Vector of (length: Natural) : Type WITH [
        empty : Vector of (zero),
        prepend length (length: Natural) value (first: Natural) tail (rest: Vector of (length))
            : Vector of (successor of (length)),
    ],
    FUNCTION (left: Natural) plus (right: Natural) : Natural
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
        "FUNCTION three is three : ((3) equals (successor of (successor of (successor of (zero)))))
            IS (reflexivity of (3)),
         FUNCTION zero is zero : ((0) equals (zero)) IS (reflexivity of (zero)),",
    )
    .unwrap();
    assert!(check("FUNCTION wrong : ((3) equals (2)) IS (reflexivity of (3)),").is_err());
}

#[test]
fn arithmetic_on_literals_computes_by_peeling() {
    check("FUNCTION sum : (((2) plus (3)) equals (5)) IS (reflexivity of (5)),").unwrap();
    assert_eq!(
        body("FUNCTION sum : Natural IS (2) plus (3),", "sum"),
        Term::Natural(5)
    );
    // A large literal stays one number: only the left side is peeled.
    assert_eq!(
        body(
            "FUNCTION big : Natural IS (1) plus (10000000000000000000000000),",
            "big"
        ),
        Term::Natural(10000000000000000000000001)
    );
}

#[test]
fn a_match_peels_a_literal() {
    assert_eq!(
        body(
            "FUNCTION predecessor of (number: Natural) : Natural
                IS (MATCH number WITH [WHEN zero THEN zero, WHEN successor of (previous) THEN previous]),
             FUNCTION two : Natural IS predecessor of (3),",
            "two"
        ),
        Term::Natural(2)
    );
}

#[test]
fn a_literal_is_an_index() {
    check(
        "FUNCTION pair : Vector of (2)
            IS (prepend length (1) value (7) tail (prepend length (0) value (8) tail (empty))),
         FUNCTION first of (vector: Vector of (2)) : Natural
            IS (MATCH vector WITH [WHEN prepend length (length) value (first) tail (rest) THEN first]),",
    )
    .unwrap();
    assert!(
        check(
            "FUNCTION pair : Vector of (3)
                IS (prepend length (1) value (7) tail (prepend length (0) value (8) tail (empty))),"
        )
        .is_err()
    );
}

#[test]
fn natural_is_built_in() {
    let error = check("DATA Natural : Type WITH [],").unwrap_err();
    assert!(error.contains("Natural"), "{error}");
    let error =
        check("FUNCTION huge : Natural IS 340282366920938463463374607431768211456,").unwrap_err();
    assert!(error.contains("too large"), "{error}");
}

#[test]
fn text_and_integer_literals_have_fixed_types() {
    check(
        "FUNCTION source : Text IS \"src\",
         FUNCTION offset : Integer IS -3,
         FUNCTION count : Integer IS 3,",
    )
    .unwrap();
    assert!(check("FUNCTION wrong : Natural IS -3,").is_err());
    assert!(check("FUNCTION wrong : Text IS 3,").is_err());
    assert!(check("FUNCTION wrong : Integer IS \"3\",").is_err());
}

#[test]
fn a_literal_type_cannot_be_matched_or_redeclared() {
    // With literals, an empty match on `Text` would prove anything.
    let error = check(
        "DATA Never : Type WITH [],
         FUNCTION absurd : Never IS (MATCH \"x\" WITH []),",
    )
    .unwrap_err();
    assert!(error.contains("absurd"), "{error}");
    for declaration in [
        "DATA Text : Type WITH [],",
        "DATA Integer : Type WITH [],",
        "DATA List of (item: Type) : Type WITH [],",
    ] {
        assert!(check(declaration).is_err(), "{declaration}");
    }
}

#[test]
fn different_literals_are_different_indices() {
    check(
        "DATA Named (name: Text) : Type WITH [named src : Named (\"src\")],
         FUNCTION src : Named (\"src\") IS named src,",
    )
    .unwrap();
    assert!(
        check(
            "DATA Named (name: Text) : Type WITH [named src : Named (\"src\")],
             FUNCTION lib : Named (\"lib\") IS named src,"
        )
        .is_err()
    );
}

#[test]
fn a_list_literal_checks_each_element() {
    check(
        "FUNCTION paths : List of (Text) IS [\"src\", \"tests\"],
         FUNCTION none : List of (Natural) IS [],
         FUNCTION nested : List of (List of (Natural)) IS [[1, 2], []],",
    )
    .unwrap();
    assert!(check("FUNCTION wrong : List of (Text) IS [\"src\", 3],").is_err());
    assert!(check("FUNCTION wrong : Text IS [\"src\"],").is_err());
}
