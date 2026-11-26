//! Functions sharing a name, such as `path of _`, told apart by a type named
//! without arguments that they take at some position.

use anyx_core::{
    kernel::{Declaration, Document, Term, TypeChecker},
    lowering::lower_document,
    parser::parse_document,
};

const RECORDS: &str = "
    RECORD Details RETURNS Type WITH [path (path: Text), size (size: Natural) bytes],
    RECORD Link RETURNS Type WITH [path (path: Text), target (target: Text)],
    FUNCTION path of (details: Details) RETURNS Text
      IS (MATCH details WITH [WHEN details (path) (size) THEN path]),
    FUNCTION path of (link: Link) RETURNS Text
      IS (MATCH link WITH [WHEN link (path) (target) THEN target]),";

fn lowered(source: &str) -> Result<Document, String> {
    let parsed = parse_document(&format!("{RECORDS}\n{source}"));
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
fn a_call_picks_the_overload_its_argument_types_name() {
    let source = r#"
        FUNCTION of details RETURNS Text IS path of { path "a.rs", size 3 bytes },
        FUNCTION of link RETURNS Text IS path of { path "b", target "a.rs" },"#;
    check(source).unwrap();
    assert_eq!(body(source, "of details"), Term::Text("a.rs".to_owned()));
    assert_eq!(body(source, "of link"), Term::Text("a.rs".to_owned()));
}

#[test]
fn the_picking_type_can_be_at_any_position() {
    let source = r#"
        DATA Plain RETURNS Type WITH [plain RETURNS Plain],
        DATA Loud RETURNS Type WITH [loud RETURNS Loud],
        FUNCTION show (text: Text) in (style: Plain) RETURNS Natural IS 1,
        FUNCTION show (text: Text) in (style: Loud) RETURNS Natural IS 2,
        FUNCTION quiet RETURNS Natural IS show "x" in (plain),
        FUNCTION shouted RETURNS Natural IS show "x" in (loud),"#;
    check(source).unwrap();
    assert_eq!(body(source, "quiet"), Term::Natural(1));
    assert_eq!(body(source, "shouted"), Term::Natural(2));
}

#[test]
fn a_parameter_picks_by_its_declared_type() {
    let source = r#"
        FUNCTION target (link: Link) RETURNS Text IS path of (link),"#;
    check(source).unwrap();
}

#[test]
fn a_literal_picks_by_its_own_type() {
    let source = r#"
        FUNCTION size of (number: Natural) RETURNS Natural IS number,
        FUNCTION size of (text: Text) RETURNS Natural IS 0,
        FUNCTION three RETURNS Natural IS size of 3,
        FUNCTION none RETURNS Natural IS size of "abc","#;
    check(source).unwrap();
    assert_eq!(body(source, "three"), Term::Natural(3));
    assert_eq!(body(source, "none"), Term::Natural(0));
}

#[test]
fn an_overload_can_recurse_through_the_shared_name() {
    let source = r#"
        FUNCTION depth of (number: Natural) RETURNS Natural IS (MATCH number WITH [
          WHEN zero THEN 0,
          WHEN successor of (smaller) THEN depth of (smaller),
        ]),
        FUNCTION depth of (text: Text) RETURNS Natural IS 0,
        FUNCTION bottom RETURNS Natural IS depth of 4,"#;
    check(source).unwrap();
    assert_eq!(body(source, "bottom"), Term::Natural(0));
}

#[test]
fn overloads_must_differ_in_a_named_type() {
    // An alias names the same type, so the two clash.
    let error = check(
        "FUNCTION Key RETURNS Type IS Text,
         FUNCTION id of (key: Key) RETURNS Text IS key,
         FUNCTION id of (text: Text) RETURNS Text IS text,",
    )
    .unwrap_err();
    assert!(error.contains("must take different named types"), "{error}");
    let error = check(
        "FUNCTION first of (texts: List of (Text)) RETURNS Natural IS 0,
         FUNCTION first of (numbers: List of (Natural)) RETURNS Natural IS 0,",
    )
    .unwrap_err();
    assert!(error.contains("each must take a named type"), "{error}");
    let error = check(
        "FUNCTION twice (text: Text) RETURNS Text IS text,
         FUNCTION twice (other: Text) RETURNS Text IS other,",
    )
    .unwrap_err();
    assert!(
        error.contains("duplicate declaration of `twice (Text)`"),
        "{error}"
    );
}

#[test]
fn a_call_no_overload_takes_is_an_error() {
    let error = check("FUNCTION wrong RETURNS Text IS path of \"x\",").unwrap_err();
    assert!(
        error.contains("no one overload of `path of _` takes `Text`"),
        "{error}"
    );
    let error = check(
        "FUNCTION size of (texts: List of (Text)) in (unit: Text) RETURNS Natural IS 0,
         FUNCTION size of (texts: List of (Text)) in (unit: Natural) RETURNS Natural IS 0,
         FUNCTION wrong RETURNS Natural IS size of [] in 3,",
    );
    assert_eq!(error, Ok(()));
    let error = check(
        "FUNCTION size of (texts: Text) in (unit: Text) RETURNS Natural IS 0,
         FUNCTION size of (texts: Natural) in (unit: Text) RETURNS Natural IS 0,
         FUNCTION wrong RETURNS Natural IS size of (MATCH 0 WITH [WHEN zero THEN 0, WHEN successor of (n) THEN n]) in \"x\",",
    )
    .unwrap_err();
    assert!(error.contains("needs a type of its own"), "{error}");
}

#[test]
fn an_overloaded_name_is_no_other_declaration() {
    let error =
        check("DATA Shape RETURNS Type WITH [path of (text: Text) RETURNS Shape],").unwrap_err();
    assert!(
        error.contains("duplicate declaration of `path of _`"),
        "{error}"
    );
    let error = check(
        "FUNCTION find (pattern: Text) [in (paths: List of (Text)) DEFAULT in []] RETURNS Text IS pattern,
         FUNCTION find (pattern: Natural) in (paths: List of (Text)) RETURNS Text IS \"x\",",
    )
    .unwrap_err();
    assert!(error.contains("so it has no optional parts"), "{error}");
}
