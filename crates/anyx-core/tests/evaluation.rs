use anyx_core::{
    kernel::{
        Case, CheckingError, Declaration, Document, EvaluationError, Global, Index, Level, Neutral,
        Pattern, Term, TypeChecker, Value, convertible, normalize, read_back,
    },
    lowering::{GlobalKind, lower_document},
    parser::parse_document,
};

fn local(index: usize) -> Term {
    Term::Local(Index(index))
}

fn function(body: Term) -> Term {
    Term::Function {
        body: Box::new(body),
    }
}

fn apply(function: Term, arguments: Vec<Term>) -> Term {
    Term::Apply {
        function: Box::new(function),
        arguments,
    }
}

#[test]
fn reads_back_captured_variables_under_new_binders() {
    let term = apply(function(function(local(1))), vec![local(0)]);
    assert_eq!(normalize(&term, 1), Ok(function(local(1))));
    assert_eq!(convertible(&term, &function(local(0)), 1), Ok(false));

    let function_type = Term::FunctionType {
        parameter_type: Box::new(local(0)),
        result: Box::new(apply(function(local(0)), vec![local(0)])),
    };
    let expected = Term::FunctionType {
        parameter_type: Box::new(local(0)),
        result: Box::new(local(0)),
    };
    assert_eq!(normalize(&function_type, 1), Ok(expected));
}

#[test]
fn conversion_ignores_neutral_application_grouping() {
    let nested = apply(apply(local(0), vec![Term::Type]), vec![local(0)]);
    let grouped = apply(local(0), vec![Term::Type, local(0)]);
    assert_eq!(normalize(&nested, 1), Ok(grouped.clone()));
    assert_eq!(convertible(&nested, &grouped, 1), Ok(true));
}

#[test]
fn evaluates_constructor_matches_from_source() {
    let parsed = parse_document(
        r#"
DATA First : Type WITH [],
DATA Second : Type WITH [],
DATA Pair : Kind WITH [pair (left: Type) with (right: Type) : Pair],
FUNCTION choose : Type IS (MATCH (pair (First) with (Second)) WITH [
    WHEN pair (left) with (right) THEN left
]),
FUNCTION expected : Type IS First,
"#,
    );
    assert!(
        parsed.syntax_errors.is_empty(),
        "{:?}",
        parsed.syntax_errors
    );
    let lowered = lower_document(&parsed.document.unwrap()).unwrap();
    let document = Document::try_from(&lowered).unwrap();
    let [
        ..,
        Declaration::Function { body, .. },
        Declaration::Function { body: expected, .. },
    ] = document.declarations.as_slice()
    else {
        panic!("expected two functions after the data declarations");
    };
    assert_eq!(
        normalize(body, 0),
        Ok(Term::Global(Global {
            name: "First".into(),
            kind: GlobalKind::Data,
        }))
    );
    assert_eq!(convertible(body, expected, 0), Ok(true));
}

#[test]
fn reads_back_blocked_matches_with_captures_and_pattern_binders() {
    let pair = Pattern::Constructor {
        name: "pair _ with _".into(),
        arguments: vec![Pattern::Binding, Pattern::Binding],
    };
    let cases = vec![
        Case {
            pattern: pair.clone(),
            result: apply(
                function(local(0)),
                vec![Term::List(vec![local(1), local(2)])],
            ),
        },
        Case {
            pattern: Pattern::Binding,
            result: local(0),
        },
    ];
    let body = Term::Match {
        subject: Box::new(local(1)),
        cases,
    };
    let term = apply(function(body), vec![Term::Type]);
    let expected = Term::Match {
        subject: Box::new(local(0)),
        cases: vec![
            Case {
                pattern: pair,
                result: Term::List(vec![local(1), Term::Type]),
            },
            Case {
                pattern: Pattern::Binding,
                result: local(0),
            },
        ],
    };
    assert_eq!(normalize(&term, 1), Ok(expected));
}

#[test]
fn blocks_on_nested_constructor_patterns_before_a_fallback() {
    let constructor = |name: &str| {
        Term::Global(Global {
            name: name.into(),
            kind: GlobalKind::Constructor,
        })
    };
    let term = Term::Match {
        subject: Box::new(apply(constructor("wrap _"), vec![local(0)])),
        cases: vec![
            Case {
                pattern: Pattern::Constructor {
                    name: "wrap _".into(),
                    arguments: vec![Pattern::Constructor {
                        name: "nothing".into(),
                        arguments: Vec::new(),
                    }],
                },
                result: constructor("nothing"),
            },
            Case {
                pattern: Pattern::Binding,
                result: constructor("one"),
            },
        ],
    };
    assert_eq!(normalize(&term, 1), Ok(term.clone()));
    assert_eq!(
        normalize(
            &apply(function(term.clone()), vec![constructor("nothing")]),
            0
        ),
        Ok(constructor("nothing"))
    );
    assert_eq!(
        normalize(&apply(function(term), vec![constructor("one")]), 0),
        Ok(constructor("one"))
    );
}

#[test]
fn reports_invalid_kernel_operations() {
    assert_eq!(
        normalize(&local(0), 0),
        Err(EvaluationError::UnboundIndex(Index(0)))
    );
    assert_eq!(
        normalize(&apply(Term::Type, vec![Term::Type]), 0),
        Err(EvaluationError::ApplyNonFunction)
    );
    assert_eq!(
        normalize(
            &Term::Match {
                subject: Box::new(Term::Type),
                cases: Vec::new(),
            },
            0,
        ),
        Err(EvaluationError::NoMatchingCase),
    );
    assert_eq!(
        read_back(&Value::Neutral(Neutral::Local(Level(0))), 0),
        Err(EvaluationError::UnboundLevel(Level(0))),
    );
    assert_eq!(
        TypeChecker::default()
            .with_global_type(
                Global {
                    name: "example".into(),
                    kind: GlobalKind::Function,
                },
                &local(0),
            )
            .err(),
        Some(CheckingError::Evaluation(EvaluationError::UnboundIndex(
            Index(0)
        ))),
    );
}
