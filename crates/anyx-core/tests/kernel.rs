use anyx_core::{
    kernel::{Declaration, Document, Index, Term},
    lowering::lower_document,
    parser::parse_document,
};

#[test]
fn indexes_parameters_patterns_and_holes_in_their_own_scopes() {
    let parsed = parse_document(
        "
DATA Pair RETURNS Kind WITH [pair (left: Type) with (right: Type) RETURNS Pair],
FUNCTION in (element: Type) choose (value: element) RETURNS ((element: element) RETURNS element)
IS (MATCH value WITH [
  WHEN pair (value) with (other) THEN (combine (value) with _ and (element)),
  WHEN fallback THEN value
]),
FUNCTION combine (first: Type) with (second: Type) and (third: Type) RETURNS Type IS first,
",
    );
    assert!(
        parsed.syntax_errors.is_empty(),
        "{:?}",
        parsed.syntax_errors
    );
    let lowered = lower_document(&parsed.document.unwrap()).unwrap();
    let document = Document::try_from(&lowered).unwrap();
    let Declaration::Function { signature, body } = &document.declarations[1] else {
        panic!("expected a function");
    };
    assert_eq!(
        signature.parameter_types,
        [Term::Type, Term::Local(Index(0))]
    );
    assert_eq!(
        signature.result,
        Term::FunctionType {
            parameter_type: Box::new(Term::Local(Index(1))),
            result: Box::new(Term::Local(Index(0))),
        }
    );

    let Term::Match { subject, cases } = body else {
        panic!("expected a match");
    };
    assert_eq!(**subject, Term::Local(Index(0)));
    let Term::Function { body } = &cases[0].result else {
        panic!("expected the hole to introduce a function");
    };
    let Term::Apply { arguments, .. } = body.as_ref() else {
        panic!("expected an application");
    };
    assert_eq!(
        arguments.as_slice(),
        [
            Term::Local(Index(2)),
            Term::Local(Index(0)),
            Term::Local(Index(4))
        ]
    );
    assert_eq!(cases[1].result, Term::Local(Index(1)));
}
