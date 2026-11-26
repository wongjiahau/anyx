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
         FUNCTION choose a natural RETURNS ((value: Natural) RETURNS Natural)
           IS (choose (Natural) value _),
         DATA Natural RETURNS Type WITH [],",
    );
    let Declaration::Function { signature, body } = &document.declarations[1] else {
        panic!("expected a function declaration");
    };
    let checker = TypeChecker::default().check_document(&document).unwrap();

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
fn rejects_mutual_recursion_without_descent() {
    let document = kernel_document(
        "FUNCTION first (element: Type) value (value: element) RETURNS element
           IS (second (element) value (value)),
         FUNCTION second (element: Type) value (value: element) RETURNS element
           IS (first (element) value (value)),",
    );
    assert!(matches!(
        TypeChecker::default().check_functions(&document),
        Err(DeclarationCheckingError::NonTerminatingFunctions(_))
    ));
}

#[test]
fn checks_forward_references_in_signatures() {
    let document = kernel_document(
        "FUNCTION identity (value: element) RETURNS element IS value,
         FUNCTION element RETURNS Type IS Natural,
         DATA Natural RETURNS Type WITH [],",
    );
    assert!(TypeChecker::default().check_document(&document).is_ok());
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
    let document = kernel_document(&format!(
        "FUNCTION singleton (item: Type) value (value: item)
           RETURNS (Vector of (item) length (successor of (zero)))
           IS (prepend (item) length (zero) value (value) tail (empty of (item))),
         {VECTOR_DATA}{MATCH_DATA}",
    ));
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
            DeclarationCheckingError::DataMustReturnUniverse("Wrong".into()),
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

const ENDPOINT_DATA: &str = "
    DATA Access RETURNS Type WITH [public RETURNS Access, bearer RETURNS Access],
    DATA Session RETURNS Type WITH [session RETURNS Session],
    DATA Credential for (access: Access) RETURNS Type WITH [
      anonymous RETURNS Credential for (public),
      verified (current: Session) RETURNS Credential for (bearer),
    ],
    DATA Health RETURNS Type WITH [ok RETURNS Health],
    DATA Profile RETURNS Type WITH [profile of (owner: Session) RETURNS Profile],
    FUNCTION Endpoint returning (response: Type) RETURNS Kind IS
      (Endpoint returning (response) access (bearer)),
    DATA Endpoint returning (response: Type) access (access: Access) RETURNS Kind WITH [
      health check RETURNS Endpoint returning (Health) access (public),
      get profile RETURNS Endpoint returning (Profile),
    ],";

#[test]
fn unfolds_an_alias_in_a_constructor_result() {
    let document = kernel_document(&format!(
        "{ENDPOINT_DATA}
         FUNCTION serve (response: Type) access (access: Access)
           at (endpoint: Endpoint returning (response) access (access))
           with (credential: Credential for (access)) RETURNS response
           IS (MATCH endpoint WITH [
             WHEN health check THEN ok,
             WHEN get profile THEN (MATCH credential WITH [
               WHEN verified (current) THEN profile of (current)]),
           ]),"
    ));
    TypeChecker::default().check_document(&document).unwrap();
}

#[test]
fn an_unfolded_constructor_result_keeps_its_indices() {
    let document = kernel_document(&format!(
        "{ENDPOINT_DATA}
         FUNCTION serve (response: Type) access (access: Access)
           at (endpoint: Endpoint returning (response) access (access))
           with (credential: Credential for (access)) RETURNS response
           IS (MATCH endpoint WITH [
             WHEN health check THEN ok,
             WHEN get profile THEN (MATCH credential WITH [
               WHEN anonymous THEN profile of (session)]),
           ]),"
    ));
    assert_eq!(
        TypeChecker::default().check_document(&document).err(),
        Some(DeclarationCheckingError::Body {
            name: "serve _ access _ at _ with _".into(),
            error: CheckingError::ImpossiblePattern("anonymous".into()),
        }),
    );
}

#[test]
fn rejects_constructor_results_that_do_not_unfold_to_their_family() {
    for (source, constructor, data) in [
        (
            "DATA Flag RETURNS Type WITH [on RETURNS Flag, off RETURNS Flag],
             DATA Other RETURNS Type WITH [],
             FUNCTION pick (flag: Flag) RETURNS Type
               IS (MATCH flag WITH [WHEN on THEN Thing, WHEN off THEN Other]),
             DATA Thing RETURNS Type WITH [bad (flag: Flag) RETURNS pick (flag)],",
            "bad _",
            "Thing",
        ),
        (
            "DATA Thing RETURNS Type WITH [bad RETURNS Loop],
             FUNCTION Loop RETURNS Type IS Loop,",
            "bad",
            "Thing",
        ),
    ] {
        assert_eq!(
            TypeChecker::default()
                .check_document(&kernel_document(source))
                .err(),
            Some(DeclarationCheckingError::ConstructorMustReturnData {
                constructor: constructor.into(),
                data: data.into(),
            }),
        );
    }
}

#[test]
fn checks_positivity_after_unfolding_a_constructor_result() {
    let document = kernel_document(
        "DATA Box of (item: Type) RETURNS Kind WITH [bad RETURNS Nested],
         FUNCTION Nested RETURNS Kind IS (Box of (Box of (Type))),",
    );
    assert_eq!(
        TypeChecker::default().check_document(&document).err(),
        Some(DeclarationCheckingError::NonPositiveOccurrence {
            declaration: "bad".into(),
            data: "Box of _".into(),
        }),
    );
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
    DATA Package RETURNS Kind WITH [
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
            CheckingError::NonExhaustiveMatch {
                pattern: "successor of (successor of (field1))".into(),
            },
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
        ("Type", "", CheckingError::MatchNeedsData),
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

#[test]
fn reports_a_missing_nested_pattern_that_can_be_added_as_a_case() {
    for (nested_case, missing_pattern) in [
        (
            "successor of (zero)",
            "successor of (successor of (field2))",
        ),
        (
            "successor of (successor of (previous))",
            "successor of (zero)",
        ),
    ] {
        let source = |added_case: &str| {
            format!(
                "{MATCH_DATA}
                 DATA Other RETURNS Type WITH [field1 RETURNS Other],
                 FUNCTION inspect (number: Natural) RETURNS Natural IS (MATCH number WITH [
                     WHEN zero THEN zero,
                     WHEN {nested_case} THEN zero,
                     {added_case}
                 ]),"
            )
        };
        let error = TypeChecker::default()
            .check_document(&kernel_document(&source("")))
            .err()
            .expect("missing case");
        assert_eq!(
            error.to_string(),
            format!(
                "body of `inspect _`: MATCH is not exhaustive (missing case: {missing_pattern})"
            )
        );
        let DeclarationCheckingError::Body {
            error: CheckingError::NonExhaustiveMatch { pattern },
            ..
        } = error
        else {
            panic!("expected a missing pattern");
        };
        let completed = source(&format!("WHEN {pattern} THEN zero,"));
        TypeChecker::default()
            .check_document(&kernel_document(&completed))
            .unwrap();
    }
}

const VECTOR_DATA: &str = "
    DATA Vector of (item: Type) length (length: Natural) RETURNS Kind WITH [
        empty of (item: Type) RETURNS (Vector of (item) length (zero)),
        prepend (item: Type) length (length: Natural) value (value: item)
            tail (tail: Vector of (item) length (length))
            RETURNS (Vector of (item) length (successor of (length))),
    ],
";

#[test]
fn checks_indexed_matches_and_refines_the_branch_result_type() {
    let document = kernel_document(&format!(
        "{MATCH_DATA}{VECTOR_DATA}
         FUNCTION copy (element: Type) length (size: Natural)
             vector (vector: Vector of (element) length (size))
             RETURNS (Vector of (element) length (size))
             IS (MATCH vector WITH [
                 WHEN empty of (item) THEN (empty of (element)),
                 WHEN prepend (item) length (length) value (value) tail (tail)
                     THEN (prepend (element) length (length) value (value) tail (tail)),
             ]),
         FUNCTION tail of (element: Type) length (size: Natural)
             vector (vector: Vector of (element) length (successor of (size)))
             RETURNS (Vector of (element) length (size))
             IS (MATCH vector WITH [
                 WHEN prepend (item) length (length) value (value) tail (tail) THEN tail,
             ]),
         FUNCTION first of a pair (vector: Vector of (Natural) length (successor of (zero)))
             RETURNS Natural IS (MATCH vector WITH [
                 WHEN prepend (item) length (length) value (zero) tail (empty of (tail item))
                     THEN zero,
                 WHEN prepend (item) length (length) value (successor of (previous))
                     tail (empty of (tail item)) THEN previous,
             ]),
         FUNCTION inspect empty (vector: Vector of (Natural) length (zero))
             RETURNS (MATCH vector WITH [WHEN empty of (item) THEN Natural])
             IS (MATCH vector WITH [WHEN empty of (item) THEN zero]),
         FUNCTION eliminate indexed (impossible: Indexed by (zero)) RETURNS Type
             IS (MATCH impossible WITH []),"
    ));
    TypeChecker::default().check_document(&document).unwrap();
}

#[test]
fn rejects_impossible_indexed_patterns_and_reports_reachable_missing_patterns() {
    for (indices, cases, expected) in [
        (
            "successor of (zero)",
            "WHEN empty of (item) THEN zero",
            CheckingError::ImpossiblePattern("empty of _".into()),
        ),
        (
            "successor of (zero)",
            "",
            CheckingError::NonExhaustiveMatch {
                pattern: "prepend (field1) length (field2) value (field3) tail (field4)".into(),
            },
        ),
        (
            "zero",
            "WHEN empty of (item) THEN zero, WHEN other THEN zero",
            CheckingError::RedundantCase(2),
        ),
        (
            "opaque of (number)",
            "WHEN empty of (item) THEN zero",
            CheckingError::NonExhaustiveMatch {
                pattern: "prepend (field1) length (field2) value (field3) tail (field4)".into(),
            },
        ),
    ] {
        let document = kernel_document(&format!(
            "{MATCH_DATA}{VECTOR_DATA}
             FUNCTION opaque of (number: Natural) RETURNS Natural IS (MATCH number WITH [
                 WHEN zero THEN zero,
                 WHEN successor of (previous) THEN (opaque of (previous)),
             ]),
             FUNCTION inspect (number: Natural) vector (vector: Vector of (Natural) length ({indices}))
                 RETURNS Natural IS (MATCH vector WITH [{cases}]),"
        ));
        assert_eq!(
            TypeChecker::default().check_document(&document).err(),
            Some(DeclarationCheckingError::Body {
                name: "inspect _ vector _".into(),
                error: expected,
            })
        );
    }
}

#[test]
fn rejects_conflicting_indices_for_a_repeated_constructor_field() {
    let document = kernel_document(&format!(
        "{MATCH_DATA}
         DATA Equal (left: Natural) to (right: Natural) RETURNS Type WITH [
             equal at (number: Natural) RETURNS (Equal (number) to (number)),
         ],
         FUNCTION eliminate (impossible: Equal (zero) to (successor of (zero)))
             RETURNS Natural IS (MATCH impossible WITH []),"
    ));
    TypeChecker::default().check_document(&document).unwrap();
}

#[test]
fn excludes_constructors_with_an_impossible_indexed_field() {
    let document = kernel_document(&format!(
        "{MATCH_DATA}{VECTOR_DATA}
         DATA At zero (index: Natural) RETURNS Type WITH [
             at zero RETURNS (At zero (zero)),
         ],
         DATA Wrapper RETURNS Type WITH [
             wrap (field: At zero (successor of (zero))) RETURNS Wrapper,
         ],
         FUNCTION eliminate wrapper (value: Wrapper) RETURNS Natural
             IS (MATCH value WITH []),
         DATA Equal (left: Natural) to (right: Natural) RETURNS Type WITH [
             equal at (number: Natural) RETURNS (Equal (number) to (number)),
         ],
         FUNCTION eliminate cycle (number: Natural)
             proof (impossible: Equal (number) to (successor of (number)))
             RETURNS Natural IS (MATCH impossible WITH []),"
    ));
    TypeChecker::default().check_document(&document).unwrap();
}

#[test]
fn confines_index_refinements_to_the_matching_branch() {
    let document = kernel_document(&format!(
        "{MATCH_DATA}{VECTOR_DATA}
         FUNCTION invalid (size: Natural) vector (vector: Vector of (Natural) length (size))
             RETURNS (Vector of (Natural) length (size))
             IS (MATCH vector WITH [
                 WHEN empty of (item) THEN (empty of (item)),
                 WHEN prepend (item) length (length) value (value) tail (tail)
                     THEN (empty of (item)),
             ]),"
    ));
    assert_eq!(
        TypeChecker::default().check_document(&document).err(),
        Some(DeclarationCheckingError::Body {
            name: "invalid _ vector _".into(),
            error: CheckingError::TypeMismatch,
        })
    );
}

#[test]
fn proves_adding_zero_on_the_right_by_induction() {
    let document = kernel_document(&format!(
        "{MATCH_DATA}
         DATA (left: Natural) equals (right: Natural) RETURNS Type WITH [
             reflexivity of (number: Natural) RETURNS ((number) equals (number)),
         ],
         FUNCTION adding zero to (number: Natural)
             RETURNS (((number) plus (zero)) equals (number))
             IS (MATCH number WITH [
                 WHEN zero THEN (reflexivity of (zero)),
                 WHEN successor of (previous) THEN
                     (successor preserves equality from ((previous) plus (zero))
                         to (previous) by (adding zero to (previous))),
             ]),
         FUNCTION successor preserves equality from (left: Natural) to (right: Natural)
             by (proof: (left) equals (right))
             RETURNS ((successor of (left)) equals (successor of (right)))
             IS (MATCH proof WITH [
                 WHEN reflexivity of (number) THEN (reflexivity of (successor of (number))),
             ]),
         FUNCTION (left: Natural) plus (right: Natural) RETURNS Natural
             IS (MATCH left WITH [
                 WHEN zero THEN right,
                 WHEN successor of (previous) THEN (successor of ((previous) plus (right))),
             ]),"
    ));
    TypeChecker::default().check_document(&document).unwrap();
}

#[test]
fn checks_mutual_structural_recursion() {
    let document = kernel_document(&format!(
        "{MATCH_DATA}
         FUNCTION even (number: Natural) RETURNS Boolean IS (MATCH number WITH [
             WHEN zero THEN true,
             WHEN successor of (previous) THEN (odd (previous)),
         ]),
         FUNCTION odd (number: Natural) RETURNS Boolean IS (MATCH number WITH [
             WHEN zero THEN false,
             WHEN successor of (previous) THEN (even (previous)),
         ]),"
    ));
    TypeChecker::default().check_document(&document).unwrap();
}

#[test]
fn rejects_recursion_on_an_unrelated_constructor_field() {
    let document = kernel_document(&format!(
        "{MATCH_DATA}
         FUNCTION recurse (first: Natural) with (second: Natural) RETURNS Natural
             IS (MATCH second WITH [
                 WHEN zero THEN zero,
                 WHEN successor of (previous) THEN (recurse (previous) with (second)),
             ]),"
    ));
    assert!(matches!(
        TypeChecker::default().check_document(&document),
        Err(DeclarationCheckingError::NonTerminatingFunctions(_))
    ));
}

#[test]
fn rejects_negative_data_hidden_behind_a_transparent_function() {
    let document = kernel_document(
        "FUNCTION alias RETURNS Type IS Recursive,
         DATA Recursive RETURNS Type WITH [
             wrap (field: ⁽(value: alias) RETURNS Type) RETURNS Recursive,
         ],",
    );
    assert!(matches!(
        TypeChecker::default().check_document(&document),
        Err(DeclarationCheckingError::NonPositiveOccurrence { .. })
    ));
}

#[test]
fn rejects_expressions_without_the_required_types() {
    for (source, expected) in [
        (
            "FUNCTION example RETURNS Type IS (successor of _)",
            CheckingError::ExpectedFunctionType,
        ),
        (
            "FUNCTION example RETURNS Natural IS (successor of (Type))",
            CheckingError::TypeMismatch,
        ),
        (
            "FUNCTION example RETURNS Natural IS (MATCH (MATCH zero WITH [WHEN value THEN value]) WITH [WHEN value THEN value])",
            CheckingError::MatchNeedsExpectedType,
        ),
        (
            "FUNCTION example RETURNS Type IS []",
            CheckingError::UnsupportedTerm,
        ),
    ] {
        let source = format!("{}{source}", include_str!("fixtures/sample.anyx"));
        assert_eq!(
            TypeChecker::default()
                .check_document(&kernel_document(&source))
                .err(),
            Some(DeclarationCheckingError::Body {
                name: "example".into(),
                error: expected,
            }),
            "{source}",
        );
    }
}

#[test]
fn rejects_malformed_kernel_patterns() {
    let mut document = kernel_document(
        "DATA Natural RETURNS Type WITH [zero RETURNS Natural, successor of (number: Natural) RETURNS Natural],
         FUNCTION example RETURNS Natural IS (MATCH zero WITH [WHEN successor of (number) THEN zero]),",
    );
    let Declaration::Function {
        body: Term::Match { cases, .. },
        ..
    } = &mut document.declarations[1]
    else {
        panic!("expected a match in the sample function");
    };
    let anyx_core::kernel::Pattern::Constructor { arguments, .. } = &mut cases[0].pattern else {
        panic!("expected a constructor pattern");
    };
    // Source names include argument slots, so an arity mismatch requires a kernel term.
    arguments.clear();
    assert_eq!(
        TypeChecker::default().check_document(&document).err(),
        Some(DeclarationCheckingError::Body {
            name: "example".into(),
            error: CheckingError::WrongPatternArgumentCount {
                constructor: "successor of _".into(),
                expected: 1,
                actual: 0,
            },
        }),
    );
}

#[test]
fn rejects_duplicate_kernel_declarations_and_unbound_references() {
    let mut document = kernel_document("FUNCTION example RETURNS Type IS Type");
    document.declarations.push(document.declarations[0].clone());
    let checker = TypeChecker::default();
    assert_eq!(
        checker.clone().check_document(&document).err(),
        Some(DeclarationCheckingError::DuplicateGlobal("example".into())),
    );
    assert_eq!(
        checker.synthesize(&Term::Local(Index(0))),
        Err(CheckingError::UnboundLocal(Index(0))),
    );
    let missing = anyx_core::kernel::Global {
        name: "missing".into(),
        kind: anyx_core::lowering::GlobalKind::Function,
    };
    assert_eq!(
        checker.synthesize(&Term::Global(missing.clone())),
        Err(CheckingError::UnknownGlobal(missing)),
    );
}

#[test]
fn locates_declaration_errors_in_signatures_bodies_and_recursive_heads() {
    for (declaration, highlighted, diagnostic) in [
        (
            "FUNCTION example RETURNS zero IS zero",
            "example RETURNS zero",
            "signature of `example`",
        ),
        (
            "FUNCTION example RETURNS Natural IS Type",
            "Type",
            "body of `example`",
        ),
        (
            "FUNCTION example RETURNS Natural IS example",
            "example",
            "cannot establish structural termination",
        ),
    ] {
        let source =
            format!("DATA Natural RETURNS Type WITH [zero RETURNS Natural],\n{declaration}");
        let parsed = parse_document(&source);
        assert!(parsed.syntax_errors.is_empty());
        let syntax = parsed.document.unwrap();
        let error = TypeChecker::default()
            .check_document(&kernel_document(&source))
            .err()
            .expect("expected the declaration to fail checking");
        let span = error.span(&syntax);
        assert_eq!(&source[span.start..span.end], highlighted);
        assert_eq!(
            span.start,
            source.find(declaration).unwrap() + declaration.find(highlighted).unwrap()
        );
        assert!(error.to_string().starts_with(diagnostic), "{error}");
    }
}

#[test]
fn separates_type_from_kind_and_checks_the_universe_of_both_function_type_parts() {
    let document = kernel_document(
        "DATA Natural RETURNS Type WITH [],
         FUNCTION universe (number: Natural) RETURNS Kind IS Type,
         FUNCTION small RETURNS Kind IS Type,
         FUNCTION higher (item: small) RETURNS Kind IS small,",
    );
    let checker = TypeChecker::default().check_document(&document).unwrap();
    let natural = Term::Global(anyx_core::kernel::Global {
        name: "Natural".into(),
        kind: anyx_core::lowering::GlobalKind::Data,
    });

    assert_eq!(checker.synthesize(&Term::Type), Ok(Term::Kind));
    assert_eq!(
        checker.synthesize(&Term::Kind),
        Err(CheckingError::KindHasNoType)
    );
    assert_eq!(
        checker.check(&Term::Type, &Term::Type),
        Err(CheckingError::TypeMismatch)
    );
    for (parameter_type, result, universe) in [
        (natural.clone(), natural.clone(), Term::Type),
        (Term::Type, natural.clone(), Term::Kind),
        (natural, Term::Type, Term::Kind),
        (Term::Type, Term::Type, Term::Kind),
    ] {
        assert_eq!(
            checker.synthesize(&Term::FunctionType {
                parameter_type: Box::new(parameter_type),
                result: Box::new(result),
            }),
            Ok(universe),
        );
    }
}

#[test]
fn constructor_fields_must_fit_the_written_data_universe() {
    for (declarations, annotation) in [
        ("", "Type"),
        ("DATA Large RETURNS Kind WITH [],", "Large"),
        ("", "((input: Type) RETURNS Type)"),
    ] {
        let rejected = kernel_document(&format!(
            "{declarations} DATA Package RETURNS Type WITH [pack (item: {annotation}) RETURNS Package],"
        ));
        assert_eq!(
            TypeChecker::default().check_document(&rejected).err(),
            Some(DeclarationCheckingError::Signature {
                name: "pack _".into(),
                error: CheckingError::ConstructorFieldMustBeInType,
            }),
        );
        let accepted = kernel_document(&format!(
            "{declarations} DATA Package RETURNS Kind WITH [pack (item: {annotation}) RETURNS Package],"
        ));
        TypeChecker::default().check_document(&accepted).unwrap();
    }
}

#[test]
fn takes_and_returns_a_kind_level_data_type() {
    let document = kernel_document(
        "DATA Package RETURNS Kind WITH [pack (item: Type) RETURNS Package],
         FUNCTION keep (package: Package) RETURNS Package IS package,
         FUNCTION package type RETURNS Kind IS Package,",
    );
    TypeChecker::default().check_document(&document).unwrap();
}
