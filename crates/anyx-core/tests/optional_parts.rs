//! `[in (paths: List of (Text)) DEFAULT in []]`: a part of a function or a
//! constructor that a call may leave out.

use anyx_core::{
    kernel::{Declaration, Document, Term, TypeChecker},
    lowering::lower_document,
    parser::parse_document,
};

const DATA: &str = "
    DATA Case : Type WITH [smart case : Case, any case : Case],
    FUNCTION find (pattern: Text)
        [in (paths: List of (Text)) DEFAULT in []]
        [with (case: Case) DEFAULT with (smart case)]
      : List of (Text),
    DATA Request : Type WITH [
        search (pattern: Text) [under (depth: Natural) DEFAULT under 3] : Request,
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
    TypeChecker::default()
        .check_document(&lowered(source)?)
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
fn a_call_that_leaves_out_parts_gets_their_defaults() {
    let source = "FUNCTION short : List of (Text) IS find \"x\",
         FUNCTION some : List of (Text) IS find \"x\" with (any case),
         FUNCTION all : List of (Text) IS find \"x\" in [\"src\"] with (smart case),
         FUNCTION written : List of (Text) IS find \"x\" in [] with (smart case),";
    check(source).unwrap();
    assert_eq!(body(source, "short"), body(source, "written"));
    assert_ne!(body(source, "some"), body(source, "written"));
}

#[test]
fn a_constructor_can_have_optional_parts() {
    let source = "FUNCTION shallow : Request IS search \"x\",
         FUNCTION written : Request IS search \"x\" under 3,";
    check(source).unwrap();
    assert_eq!(body(source, "shallow"), body(source, "written"));
}

#[test]
fn a_default_is_checked_and_names_no_parameter() {
    let error = check(
        "FUNCTION wrong (pattern: Text) [in (paths: List of (Text)) DEFAULT in 3] : Text IS pattern,",
    )
    .unwrap_err();
    assert!(error.contains("default paths of wrong"), "{error}");
    let error = check(
        "FUNCTION wrong (item: Type) [holding (value: item) DEFAULT holding 3] : Type IS item,",
    )
    .unwrap_err();
    assert!(error.contains("depends on the parameter `item`"), "{error}");
}

#[test]
fn only_functions_and_constructors_have_optional_parts() {
    for source in [
        "DATA Box [of (item: Type) DEFAULT of Text] : Type WITH [],",
        "RECORD Pair : Type WITH [first (x: Text) [then (y: Text) DEFAULT then \"\"]],",
    ] {
        assert!(check(source).is_err(), "{source}");
    }
    // A shorter form can't be another declaration's name.
    let error = check("FUNCTION find (pattern: Text) : Text IS pattern,").unwrap_err();
    assert!(error.contains("find _"), "{error}");
}
