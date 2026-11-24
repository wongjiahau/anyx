use anyx_core::{
    kernel::{CheckingError, Declaration, Document, Global, Index, Term, TypeChecker},
    lowering::{GlobalKind, lower_document},
    parser::parse_document,
};

#[test]
fn checks_a_hole_against_a_dependent_function_type() {
    let parsed = parse_document(
        "FUNCTION choose (element: Type) value (value: element) RETURNS element IS value,
         FUNCTION choose a type RETURNS ((value: Type) RETURNS Type) IS (choose (Type) value _),",
    );
    assert!(
        parsed.syntax_errors.is_empty(),
        "{:?}",
        parsed.syntax_errors
    );
    let lowered = lower_document(&parsed.document.unwrap()).unwrap();
    let document = Document::try_from(&lowered).unwrap();
    let [
        Declaration::Function { signature, .. },
        Declaration::Function {
            signature: example,
            body,
        },
    ] = document.declarations.as_slice()
    else {
        panic!("expected two function declarations");
    };
    let mut global_type = signature.result.clone();
    for parameter_type in signature.parameter_types.iter().rev() {
        global_type = Term::FunctionType {
            parameter_type: Box::new(parameter_type.clone()),
            result: Box::new(global_type),
        };
    }
    let checker = TypeChecker::default()
        .with_global_type(
            Global {
                name: signature.name.clone(),
                kind: GlobalKind::Function,
            },
            &global_type,
        )
        .unwrap();

    assert_eq!(checker.check(body, &example.result), Ok(()));
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
