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
        "FUNCTION choose (element: Type) value (value: element) : element IS value,
         FUNCTION choose a natural : ((value: Natural) -> Natural)
           IS (choose (Natural) value _),",
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
        "FUNCTION first (element: Type) value (value: element) : element
           IS (second (element) value (value)),
         FUNCTION second (element: Type) value (value: element) : element
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
        "FUNCTION identity (value: element) : element IS value,
         FUNCTION element : Type IS Natural,",
    );
    assert!(TypeChecker::default().check_document(&document).is_ok());
}

#[test]
fn rejects_an_ill_formed_signature() {
    let document = kernel_document(
        "FUNCTION invalid (element: Type) value (value: element) : value IS value,",
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
        "FUNCTION invalid (element: Type) value (value: element) : element IS Type,",
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
           : (Vector of (item) length (successor of (zero)))
           IS (prepend (item) length (zero) value (value) tail (empty of (item))),
         {VECTOR_DATA}{MATCH_DATA}",
    ));
    TypeChecker::default().check_document(&document).unwrap();
}

#[test]
fn accepts_positive_mutual_recursion_and_nonrecursive_function_domains() {
    let document = kernel_document(
        "DATA First : Type WITH [
           first from (callback: ((input: Unrelated) -> Second)) : First],
         DATA Second : Type WITH [second from (previous: First) : Second],
         DATA Unrelated : Type WITH [],",
    );
    TypeChecker::default().check_document(&document).unwrap();
}

#[test]
fn rejects_invalid_data_and_constructor_results() {
    for (source, expected) in [
        (
            "DATA Wrong : Wrong WITH [],",
            DeclarationCheckingError::DataMustReturnUniverse("Wrong".into()),
        ),
        (
            "DATA First : Type WITH [wrong : Second],
             DATA Second : Type WITH [],",
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
    DATA Access : Type WITH [public : Access, bearer : Access],
    DATA Session : Type WITH [session : Session],
    DATA Credential for (access: Access) : Type WITH [
      anonymous : Credential for (public),
      verified (current: Session) : Credential for (bearer),
    ],
    DATA Health : Type WITH [ok : Health],
    DATA Profile : Type WITH [profile of (owner: Session) : Profile],
    FUNCTION Endpoint returning (response: Type) : Kind IS
      (Endpoint returning (response) access (bearer)),
    DATA Endpoint returning (response: Type) access (access: Access) : Kind WITH [
      health check : Endpoint returning (Health) access (public),
      get profile : Endpoint returning (Profile),
    ],";

#[test]
fn unfolds_an_alias_in_a_constructor_result() {
    let document = kernel_document(&format!(
        "{ENDPOINT_DATA}
         FUNCTION serve (response: Type) access (access: Access)
           at (endpoint: Endpoint returning (response) access (access))
           with (credential: Credential for (access)) : response
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
           with (credential: Credential for (access)) : response
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
            "DATA Flag : Type WITH [on : Flag, off : Flag],
             DATA Other : Type WITH [],
             FUNCTION pick (flag: Flag) : Type
               IS (MATCH flag WITH [WHEN on THEN Thing, WHEN off THEN Other]),
             DATA Thing : Type WITH [bad (flag: Flag) : pick (flag)],",
            "bad _",
            "Thing",
        ),
        (
            "DATA Thing : Type WITH [bad : Loop],
             FUNCTION Loop : Type IS Loop,",
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
        "DATA Box of (item: Type) : Kind WITH [bad : Nested],
         FUNCTION Nested : Kind IS (Box of (Box of (Type))),",
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
fn a_parameter_does_not_count_towards_the_universe() {
    let document =
        kernel_document("DATA Folder : Type WITH [folder with (files: List of (Text)) : Folder],");
    TypeChecker::default().check_document(&document).unwrap();
}

#[test]
fn a_large_field_that_is_not_a_uniform_parameter_is_rejected() {
    for source in [
        "DATA Any : Type WITH [pack (code: Type) : Any],",
        // The recursive occurrence changes the argument, so it is an index.
        "DATA Weird of (item: Type) : Type WITH [
           stop of (item: Type) : Weird of (item),
           step of (item: Type) inner (rest: Weird of (Text)) : Weird of (item),
         ],",
        // One constructor fixes the argument, so it is an index.
        "DATA Tagged (item: Type) : Type WITH [
           text tag : Tagged (Text),
           any tag (item: Type) : Tagged (item),
         ],",
    ] {
        assert!(matches!(
            TypeChecker::default().check_document(&kernel_document(source)),
            Err(DeclarationCheckingError::Signature {
                error: CheckingError::ConstructorFieldMustBeInType,
                ..
            })
        ));
    }
}

#[test]
fn a_served_function_is_an_opaque_constant() {
    let document = kernel_document(
        "DATA Document : Type WITH [document of (text: Text) : Document],
         FUNCTION read (path: Text) : Document,
         FUNCTION read twice (path: Text) : Document IS (read (path)),",
    );
    TypeChecker::default().check_document(&document).unwrap();
}

#[test]
fn a_served_function_cannot_return_a_type() {
    for source in [
        "FUNCTION make type : Type,",
        "DATA Big : Kind WITH [big (code: Type) : Big],
         FUNCTION make big : Big,",
    ] {
        assert!(matches!(
            TypeChecker::default().check_document(&kernel_document(source)),
            Err(DeclarationCheckingError::ServedFunctionMustReturnAValue(_))
        ));
    }
}

#[test]
fn rejects_negative_and_nested_recursive_occurrences() {
    for (source, reference) in [
        (
            "DATA First : Type WITH [
               bad (callback: ((nested: ((input: First) -> Type)) -> Type))
                 : First],",
            "First",
        ),
        (
            "DATA First : Type WITH [
               bad (callback: ((input: Second) -> Type)) : First],
             DATA Second : Type WITH [second from (previous: First) : Second],",
            "Second",
        ),
        (
            "DATA Box of (item: Type) : Type WITH [],
             DATA First : Type WITH [
               bad (nested: Box of (First)) : First],",
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
    DATA Boolean : Type WITH [false : Boolean, true : Boolean],
    DATA Empty : Type WITH [],
    DATA Package : Kind WITH [
        pack (element: Type) value (value: element) : Package,
    ],
    DATA Indexed by (index: Natural) : Type WITH [],
";

#[test]
fn checks_nested_patterns_and_dependent_fields() {
    let document = kernel_document(&format!(
        "FUNCTION inspect (number: Natural) : Natural IS (MATCH number WITH [
            WHEN zero THEN zero,
            WHEN successor of (zero) THEN zero,
            WHEN successor of (successor of (previous)) THEN previous,
        ]),
        FUNCTION consume (package: Package) : Natural IS (MATCH package WITH [
            WHEN pack (element) value (value) THEN (use (element) value (value)),
        ]),
        FUNCTION use (element: Type) value (value: element) : Natural IS zero,
        {MATCH_DATA}"
    ));
    TypeChecker::default().check_document(&document).unwrap();
}

#[test]
fn checks_an_empty_match_and_normalizes_it_under_a_binder() {
    let document = kernel_document(&format!(
        "FUNCTION eliminate (impossible: Empty) : Type IS (MATCH impossible WITH []),
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
             FUNCTION inspect (subject: {subject_type}) : Natural
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
                 DATA Other : Type WITH [field1 : Other],
                 FUNCTION inspect (number: Natural) : Natural IS (MATCH number WITH [
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
    DATA Vector of (item: Type) length (length: Natural) : Kind WITH [
        empty of (item: Type) : (Vector of (item) length (zero)),
        prepend (item: Type) length (length: Natural) value (value: item)
            tail (tail: Vector of (item) length (length))
            : (Vector of (item) length (successor of (length))),
    ],
";

#[test]
fn checks_indexed_matches_and_refines_the_branch_result_type() {
    let document = kernel_document(&format!(
        "{MATCH_DATA}{VECTOR_DATA}
         FUNCTION copy (element: Type) length (size: Natural)
             vector (vector: Vector of (element) length (size))
             : (Vector of (element) length (size))
             IS (MATCH vector WITH [
                 WHEN empty of (item) THEN (empty of (element)),
                 WHEN prepend (item) length (length) value (value) tail (tail)
                     THEN (prepend (element) length (length) value (value) tail (tail)),
             ]),
         FUNCTION tail of (element: Type) length (size: Natural)
             vector (vector: Vector of (element) length (successor of (size)))
             : (Vector of (element) length (size))
             IS (MATCH vector WITH [
                 WHEN prepend (item) length (length) value (value) tail (tail) THEN tail,
             ]),
         FUNCTION first of a pair (vector: Vector of (Natural) length (successor of (zero)))
             : Natural IS (MATCH vector WITH [
                 WHEN prepend (item) length (length) value (zero) tail (empty of (tail item))
                     THEN zero,
                 WHEN prepend (item) length (length) value (successor of (previous))
                     tail (empty of (tail item)) THEN previous,
             ]),
         FUNCTION inspect empty (vector: Vector of (Natural) length (zero))
             : (MATCH vector WITH [WHEN empty of (item) THEN Natural])
             IS (MATCH vector WITH [WHEN empty of (item) THEN zero]),
         FUNCTION eliminate indexed (impossible: Indexed by (zero)) : Type
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
             FUNCTION opaque of (number: Natural) : Natural IS (MATCH number WITH [
                 WHEN zero THEN zero,
                 WHEN successor of (previous) THEN (opaque of (previous)),
             ]),
             FUNCTION inspect (number: Natural) vector (vector: Vector of (Natural) length ({indices}))
                 : Natural IS (MATCH vector WITH [{cases}]),"
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
         DATA Equal (left: Natural) to (right: Natural) : Type WITH [
             equal at (number: Natural) : (Equal (number) to (number)),
         ],
         FUNCTION eliminate (impossible: Equal (zero) to (successor of (zero)))
             : Natural IS (MATCH impossible WITH []),"
    ));
    TypeChecker::default().check_document(&document).unwrap();
}

#[test]
fn excludes_constructors_with_an_impossible_indexed_field() {
    let document = kernel_document(&format!(
        "{MATCH_DATA}{VECTOR_DATA}
         DATA At zero (index: Natural) : Type WITH [
             at zero : (At zero (zero)),
         ],
         DATA Wrapper : Type WITH [
             wrap (field: At zero (successor of (zero))) : Wrapper,
         ],
         FUNCTION eliminate wrapper (value: Wrapper) : Natural
             IS (MATCH value WITH []),
         DATA Equal (left: Natural) to (right: Natural) : Type WITH [
             equal at (number: Natural) : (Equal (number) to (number)),
         ],
         FUNCTION eliminate cycle (number: Natural)
             proof (impossible: Equal (number) to (successor of (number)))
             : Natural IS (MATCH impossible WITH []),"
    ));
    TypeChecker::default().check_document(&document).unwrap();
}

#[test]
fn confines_index_refinements_to_the_matching_branch() {
    let document = kernel_document(&format!(
        "{MATCH_DATA}{VECTOR_DATA}
         FUNCTION invalid (size: Natural) vector (vector: Vector of (Natural) length (size))
             : (Vector of (Natural) length (size))
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
         DATA (left: Natural) equals (right: Natural) : Type WITH [
             reflexivity of (number: Natural) : ((number) equals (number)),
         ],
         FUNCTION adding zero to (number: Natural)
             : (((number) plus (zero)) equals (number))
             IS (MATCH number WITH [
                 WHEN zero THEN (reflexivity of (zero)),
                 WHEN successor of (previous) THEN
                     (successor preserves equality from ((previous) plus (zero))
                         to (previous) by (adding zero to (previous))),
             ]),
         FUNCTION successor preserves equality from (left: Natural) to (right: Natural)
             by (proof: (left) equals (right))
             : ((successor of (left)) equals (successor of (right)))
             IS (MATCH proof WITH [
                 WHEN reflexivity of (number) THEN (reflexivity of (successor of (number))),
             ]),
         FUNCTION (left: Natural) plus (right: Natural) : Natural
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
         FUNCTION even (number: Natural) : Boolean IS (MATCH number WITH [
             WHEN zero THEN true,
             WHEN successor of (previous) THEN (odd (previous)),
         ]),
         FUNCTION odd (number: Natural) : Boolean IS (MATCH number WITH [
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
         FUNCTION recurse (first: Natural) with (second: Natural) : Natural
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
        "FUNCTION alias : Type IS Recursive,
         DATA Recursive : Type WITH [
             wrap (field: ⁽(value: alias) -> Type) : Recursive,
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
            "FUNCTION example : Type IS (successor of _)",
            CheckingError::ExpectedFunctionType,
        ),
        (
            "FUNCTION example : Natural IS (successor of (Type))",
            CheckingError::TypeMismatch,
        ),
        (
            "FUNCTION example : Natural IS (MATCH (MATCH zero WITH [WHEN value THEN value]) WITH [WHEN value THEN value])",
            CheckingError::MatchNeedsExpectedType,
        ),
        ("FUNCTION example : Type IS []", CheckingError::TypeMismatch),
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
        "FUNCTION example : Natural IS (MATCH zero WITH [WHEN successor of (number) THEN zero]),",
    );
    let Declaration::Function {
        body: Term::Match { cases, .. },
        ..
    } = &mut document.declarations[0]
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
    let mut document = kernel_document("FUNCTION example : Type IS Type");
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
            "FUNCTION example : zero IS zero",
            "example : zero",
            "signature of `example`",
        ),
        (
            "FUNCTION example : Natural IS Type",
            "Type",
            "body of `example`",
        ),
        (
            "FUNCTION example : Natural IS example",
            "example",
            "cannot establish structural termination",
        ),
    ] {
        let source = declaration.to_owned();
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
        "FUNCTION universe (number: Natural) : Kind IS Type,
         FUNCTION small : Kind IS Type,
         FUNCTION higher (item: small) : Kind IS small,",
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
        ("DATA Large : Kind WITH [],", "Large"),
        ("", "((input: Type) -> Type)"),
    ] {
        let rejected = kernel_document(&format!(
            "{declarations} DATA Package : Type WITH [pack (item: {annotation}) : Package],"
        ));
        assert_eq!(
            TypeChecker::default().check_document(&rejected).err(),
            Some(DeclarationCheckingError::Signature {
                name: "pack _".into(),
                error: CheckingError::ConstructorFieldMustBeInType,
            }),
        );
        let accepted = kernel_document(&format!(
            "{declarations} DATA Package : Kind WITH [pack (item: {annotation}) : Package],"
        ));
        TypeChecker::default().check_document(&accepted).unwrap();
    }
}

#[test]
fn takes_and_returns_a_kind_level_data_type() {
    let document = kernel_document(
        "DATA Package : Kind WITH [pack (item: Type) : Package],
         FUNCTION keep (package: Package) : Package IS package,
         FUNCTION package type : Kind IS Package,",
    );
    TypeChecker::default().check_document(&document).unwrap();
}
