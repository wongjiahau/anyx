use anyx_core::{
    kernel::{
        CheckingError, Declaration, DeclarationCheckingError, Document, FunctionCheckingError,
        Index, Term, TypeChecker,
    },
    lowering::lower_document,
    parser::parse_document,
};

#[test]
fn checks_a_hole_against_a_dependent_function_type() {
    let document = kernel_document(
        "FUNCTION choose (element: Type) value (value: element) RETURNS element IS value,
         FUNCTION choose a type RETURNS ((value: Type) RETURNS Type) IS (choose (Type) value _),",
    );
    let Declaration::Function { signature, body } = &document.declarations[1] else {
        panic!("expected a function declaration");
    };
    let checker = TypeChecker::default().check_functions(&document).unwrap();

    assert_eq!(checker.check(body, &signature.result), Ok(()));
    assert_eq!(
        checker.synthesize(body),
        Err(CheckingError::FunctionNeedsExpectedType)
    );
}

#[test]
fn rejects_using_a_value_where_a_type_is_required() {
    let function_type = Term::FunctionType {
        parameter_type: Box::new(Term::Type),
        result: Box::new(Term::FunctionType {
            parameter_type: Box::new(Term::Local(Index(0))),
            result: Box::new(Term::Local(Index(0))),
        }),
    };
    assert_eq!(
        TypeChecker::default().synthesize(&function_type),
        Err(CheckingError::TypeMismatch)
    );
}

#[test]
fn checks_mutual_recursion_with_dependent_signatures() {
    let document = kernel_document(
        "FUNCTION first (element: Type) value (value: element) RETURNS element
           IS (second (element) value (value)),
         FUNCTION second (element: Type) value (value: element) RETURNS element
           IS (first (element) value (value)),",
    );
    assert!(TypeChecker::default().check_functions(&document).is_ok());
}

#[test]
fn checks_forward_references_in_signatures() {
    let document = kernel_document(
        "FUNCTION identity (value: element) RETURNS element IS value,
         FUNCTION element RETURNS Type IS Type,",
    );
    assert!(TypeChecker::default().check_functions(&document).is_ok());
}

#[test]
fn rejects_an_ill_formed_signature() {
    let document = kernel_document(
        "FUNCTION invalid (element: Type) value (value: element) RETURNS value IS value,",
    );
    assert!(matches!(
        TypeChecker::default().check_functions(&document),
        Err(FunctionCheckingError::Signature {
            error: CheckingError::TypeMismatch,
            ..
        })
    ));
}

#[test]
fn rejects_a_body_that_disagrees_with_its_declared_type() {
    let document = kernel_document(
        "FUNCTION invalid (element: Type) value (value: element) RETURNS element IS Type,",
    );
    assert!(matches!(
        TypeChecker::default().check_functions(&document),
        Err(FunctionCheckingError::Body {
            error: CheckingError::TypeMismatch,
            ..
        })
    ));
}

#[test]
fn checks_data_families_and_exposes_constructors_to_functions() {
    let document = kernel_document(
        "FUNCTION singleton (item: Type) value (value: item)
           RETURNS (Vector of (item) length (successor of (zero)))
           IS (prepend (item) length (zero) value (value) tail (empty of (item))),
         DATA Vector of (item: Type) length (length: Natural) RETURNS Type WITH [
           empty of (item: Type) RETURNS (Vector of (item) length (zero)),
           prepend (item: Type) length (length: Natural) value (value: item)
             tail (tail: Vector of (item) length (length))
             RETURNS (Vector of (item) length (successor of (length))),
         ],
         DATA Natural RETURNS Type WITH [zero RETURNS Natural,
           successor of (previous: Natural) RETURNS Natural],",
    );
    TypeChecker::default().check_document(&document).unwrap();
}

#[test]
fn accepts_positive_mutual_recursion_and_nonrecursive_function_domains() {
    let document = kernel_document(
        "DATA First RETURNS Type WITH [
           first from (callback: ((input: Unrelated) RETURNS Second)) RETURNS First],
         DATA Second RETURNS Type WITH [second from (previous: First) RETURNS Second],
         DATA Unrelated RETURNS Type WITH [],",
    );
    TypeChecker::default().check_document(&document).unwrap();
}

#[test]
fn rejects_invalid_data_and_constructor_results() {
    for (source, expected) in [
        (
            "DATA Wrong RETURNS Wrong WITH [],",
            DeclarationCheckingError::DataMustReturnType("Wrong".into()),
        ),
        (
            "DATA First RETURNS Type WITH [wrong RETURNS Second],
             DATA Second RETURNS Type WITH [],",
            DeclarationCheckingError::ConstructorMustReturnData {
                constructor: "wrong".into(),
                data: "First".into(),
            },
        ),
    ] {
        assert_eq!(
            TypeChecker::default()
                .check_document(&kernel_document(source))
                .err(),
            Some(expected),
        );
    }
}

#[test]
fn rejects_negative_and_nested_recursive_occurrences() {
    for (source, reference) in [
        (
            "DATA First RETURNS Type WITH [
               bad (callback: ((nested: ((input: First) RETURNS Type)) RETURNS Type))
                 RETURNS First],",
            "First",
        ),
        (
            "DATA First RETURNS Type WITH [
               bad (callback: ((input: Second) RETURNS Type)) RETURNS First],
             DATA Second RETURNS Type WITH [second from (previous: First) RETURNS Second],",
            "Second",
        ),
        (
            "DATA Box of (item: Type) RETURNS Type WITH [],
             DATA First RETURNS Type WITH [
               bad (nested: Box of (First)) RETURNS First],",
            "First",
        ),
    ] {
        assert_eq!(
            TypeChecker::default()
                .check_document(&kernel_document(source))
                .err(),
            Some(DeclarationCheckingError::NonPositiveOccurrence {
                declaration: "bad _".into(),
                data: reference.into(),
            }),
        );
    }
}

fn kernel_document(source: &str) -> Document {
    let parsed = parse_document(source);
    assert!(
        parsed.syntax_errors.is_empty(),
        "{:?}",
        parsed.syntax_errors
    );
    let lowered = lower_document(&parsed.document.unwrap()).unwrap();
    Document::try_from(&lowered).unwrap()
}

const MATCH_DATA: &str = "
    DATA Natural RETURNS Type WITH [
        zero RETURNS Natural,
        successor of (previous: Natural) RETURNS Natural,
    ],
    DATA Boolean RETURNS Type WITH [false RETURNS Boolean, true RETURNS Boolean],
    DATA Empty RETURNS Type WITH [],
    DATA Package RETURNS Type WITH [
        pack (element: Type) value (value: element) RETURNS Package,
    ],
    DATA Indexed by (index: Natural) RETURNS Type WITH [],
";

#[test]
fn checks_nested_patterns_and_dependent_fields() {
    let document = kernel_document(&format!(
        "FUNCTION inspect (number: Natural) RETURNS Natural IS (MATCH number WITH [
            WHEN zero THEN zero,
            WHEN successor of (zero) THEN zero,
            WHEN successor of (successor of (previous)) THEN previous,
        ]),
        FUNCTION consume (package: Package) RETURNS Natural IS (MATCH package WITH [
            WHEN pack (element) value (value) THEN (use (element) value (value)),
        ]),
        FUNCTION use (element: Type) value (value: element) RETURNS Natural IS zero,
        {MATCH_DATA}"
    ));
    TypeChecker::default().check_document(&document).unwrap();
}

#[test]
fn checks_an_empty_match_and_normalizes_it_under_a_binder() {
    let document = kernel_document(&format!(
        "FUNCTION eliminate (impossible: Empty) RETURNS Type IS (MATCH impossible WITH []),
         {MATCH_DATA}"
    ));
    TypeChecker::default().check_document(&document).unwrap();
    let Declaration::Function { body, .. } = &document.declarations[0] else {
        panic!("expected a function declaration");
    };
    let function = Term::Function {
        body: Box::new(body.clone()),
    };
    assert_eq!(
        anyx_core::kernel::normalize(&function, 0),
        Ok(function.clone())
    );
}

#[test]
fn rejects_invalid_plain_data_matches() {
    for (subject_type, cases, expected) in [
        (
            "Natural",
            "WHEN successor of (zero) THEN zero, WHEN zero THEN zero",
            CheckingError::NonExhaustiveMatch,
        ),
        (
            "Natural",
            "WHEN zero THEN zero, WHEN successor of (previous) THEN previous, WHEN rest THEN rest",
            CheckingError::RedundantCase(3),
        ),
        (
            "Natural",
            "WHEN successor of (previous) THEN previous, WHEN successor of (zero) THEN zero",
            CheckingError::RedundantCase(2),
        ),
        (
            "Natural",
            "WHEN false THEN zero",
            CheckingError::WrongPatternConstructor("false".into()),
        ),
        (
            "Natural",
            "WHEN zero THEN Type, WHEN successor of (previous) THEN previous",
            CheckingError::TypeMismatch,
        ),
        (
            "Empty",
            "WHEN impossible THEN zero",
            CheckingError::RedundantCase(1),
        ),
        ("Type", "", CheckingError::MatchNeedsPlainData),
        ("Indexed by (zero)", "", CheckingError::MatchNeedsPlainData),
    ] {
        let document = kernel_document(&format!(
            "{MATCH_DATA}
             FUNCTION inspect (subject: {subject_type}) RETURNS Natural
                 IS (MATCH subject WITH [{cases}]),"
        ));
        assert_eq!(
            TypeChecker::default().check_document(&document).err(),
            Some(DeclarationCheckingError::Body {
                name: "inspect _".into(),
                error: expected
            })
        );
    }
}
