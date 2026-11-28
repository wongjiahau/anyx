//! Overloads told apart at any type position, the result included: two
//! types may both declare `red`, and the expected type picks.

use anyx_core::{
    kernel::{Declaration, Document, Term, TypeChecker},
    lowering::lower_document,
    parser::parse_document,
};

const COLOURS: &str = "
    DATA Colour WITH [red, green],
    DATA Light WITH [red, amber],
    DATA Canvas WITH [canvas],
    FUNCTION paint (colour: Colour) : Canvas IS canvas,
    FUNCTION paint (light: Light) : Canvas IS canvas,";

fn lowered(source: &str) -> Result<Document, String> {
    let source = format!("{COLOURS}\n{source}");
    let parsed = parse_document(&source);
    assert!(
        parsed.syntax_errors.is_empty(),
        "{:?}",
        parsed.syntax_errors
    );
    let document = parsed
        .document
        .map_err(|error| error.render(&source, "x"))?;
    let lowered = lower_document(&document).map_err(|error| error.message)?;
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

fn constructor(name: &str) -> Term {
    Term::Global(anyx_core::kernel::Global {
        name: name.to_owned(),
        kind: anyx_core::lowering::GlobalKind::Constructor,
    })
}

#[test]
fn two_types_may_declare_the_same_constructor() {
    let source = "
        FUNCTION c : Colour IS red,
        FUNCTION l : Light IS red,";
    assert_eq!(check(source), Ok(()));
    assert_eq!(body(source, "c"), constructor("red : Colour"));
    assert_eq!(body(source, "l"), constructor("red : Light"));
}

#[test]
fn an_argument_of_a_picked_call_has_an_expected_type() {
    let source = "
        FUNCTION shade (colour: Colour) : Colour IS colour,
        FUNCTION s : Colour IS shade (red),";
    assert_eq!(check(source), Ok(()));
}

#[test]
fn without_an_expected_type_an_ascription_says_which() {
    let error = check("FUNCTION p : Canvas IS paint (red),").unwrap_err();
    assert!(error.contains("write one with an ascription"), "{error}");
    assert_eq!(
        check("FUNCTION p : Canvas IS paint (red : Colour),"),
        Ok(())
    );
    let error =
        check("FUNCTION r : Colour IS (MATCH red WITH [WHEN red THEN green]),").unwrap_err();
    assert!(
        error.contains("`red` could be `red : Colour` or `red : Light`"),
        "{error}"
    );
}

#[test]
fn a_function_may_be_overloaded_on_its_result_alone() {
    let source = r#"
        FUNCTION parse (text: Text) : Colour IS red,
        FUNCTION parse (text: Text) : Light IS amber,
        FUNCTION c : Colour IS parse "x",
        FUNCTION l : Light IS parse "x","#;
    assert_eq!(check(source), Ok(()));
    assert_eq!(body(source, "c"), constructor("red : Colour"));
    assert_eq!(body(source, "l"), constructor("amber"));
}

#[test]
fn overloads_must_be_apart_somewhere() {
    let error = check(
        "FUNCTION same (text: Text) : Colour IS red,
         FUNCTION same (other: Text) : Colour IS green,",
    )
    .unwrap_err();
    assert!(
        error.contains("duplicate") || error.contains("overload"),
        "{error}"
    );
    // A variable's type tells nothing apart.
    let error = check(
        "FUNCTION pick (t: Type) (x: t) : Colour IS red,
         FUNCTION pick (u: Type) (y: u) : Colour IS green,",
    )
    .unwrap_err();
    assert!(
        error.contains("duplicate") || error.contains("overload"),
        "{error}"
    );
}

#[test]
fn a_type_application_tells_overloads_apart() {
    assert_eq!(
        check(
            "FUNCTION size of (colours: List of (Colour)) : Natural IS 0,
             FUNCTION size of (lights: List of (Light)) : Natural IS 1,
             FUNCTION n (colours: List of (Colour)) : Natural IS size of (colours),"
        ),
        Ok(())
    );
}

#[test]
fn a_pattern_finds_its_constructor_in_the_type_matched() {
    let source = "
        DATA Boolean WITH [true, false],
        FUNCTION warm (colour: Colour) : Boolean
          IS (MATCH colour WITH [WHEN red THEN true, WHEN green THEN false]),
        FUNCTION lit (light: Light) : Boolean
          IS (MATCH light WITH [WHEN red THEN false, WHEN amber THEN true]),
        FUNCTION w : Boolean IS warm (red),";
    assert_eq!(check(source), Ok(()));
    assert_eq!(body(source, "w"), constructor("true"));
}

#[test]
fn one_type_has_one_constructor_of_a_name() {
    assert!(
        check("DATA Shape WITH [circle (r: Natural), circle (d: Text)],")
            .unwrap_err()
            .contains("two constructors called `circle _`")
    );
}

const SIZES: &str = "
    RECORD Size WITH [width (w: Natural), height (h: Natural)],
    RECORD Gap WITH [width (w: Natural), height (h: Natural)],
    FUNCTION area of (size: Size) : Natural IS 1,
    FUNCTION area of (gap: Gap) : Natural IS 2,";

#[test]
fn the_expected_type_picks_among_records_with_the_same_fields() {
    let source = format!(
        "{SIZES}
         FUNCTION s : Size IS {{ width 3, height 4 }},
         FUNCTION g : Gap IS {{ width 3, height 4 }},
         FUNCTION n : Natural IS area of ({{ width 3, height 4 }} : Gap),"
    );
    assert_eq!(check(&source), Ok(()));
    assert_eq!(body(&source, "n"), Term::Natural(2));
}

#[test]
fn a_record_value_without_an_expected_type_says_which() {
    let error = check(&format!(
        "{SIZES} FUNCTION n : Natural IS area of {{ width 3, height 4 }},"
    ))
    .unwrap_err();
    assert!(error.contains("ascription"), "{error}");
}

#[test]
fn a_dot_entry_may_be_an_overloaded_constructor() {
    assert_eq!(
        check(
            "RECORD Paint WITH [.(colour: Colour), coats (n: Natural)],
             FUNCTION p : Paint IS { .red, coats 2 },"
        ),
        Ok(())
    );
}
