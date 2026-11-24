use anyx_core::{
    lowering::{Declaration, Document, GlobalKind, LoweringError, Pattern, Term, lower_document},
    parser::parse_document,
};

fn lower(source: &str) -> Result<Document, LoweringError> {
    let parsed = parse_document(source);
    assert!(
        parsed.syntax_errors.is_empty(),
        "{:?}",
        parsed.syntax_errors
    );
    lower_document(&parsed.document.unwrap())
}

#[test]
fn resolves_forward_references_and_match_bindings() {
    let document = lower(
        "
FUNCTION predecessor of (number: Natural) RETURNS Natural IS (MATCH number WITH [WHEN zero THEN zero, WHEN successor of (number) THEN number]),
DATA Natural RETURNS Type WITH [zero RETURNS Natural, successor of (number: Natural) RETURNS Natural],
",
    )
    .unwrap();
    let Declaration::Function { signature, body } = &document.declarations[0] else {
        panic!("expected a function");
    };
    assert_eq!(
        signature.result,
        Term::Global {
            name: "Natural".to_owned(),
            kind: GlobalKind::Data,
        }
    );
    let Term::Match { subject, cases } = body else {
        panic!("expected a match");
    };
    assert_eq!(
        **subject,
        Term::Local(signature.parameters[0].binding.identifier)
    );
    assert_eq!(
        cases[0].pattern,
        Pattern::Constructor {
            name: "zero".to_owned(),
            arguments: vec![],
        }
    );
    assert_eq!(
        cases[0].result,
        Term::Global {
            name: "zero".to_owned(),
            kind: GlobalKind::Constructor,
        }
    );
    let Pattern::Constructor { name, arguments } = &cases[1].pattern else {
        panic!("expected a constructor pattern");
    };
    assert_eq!(name, "successor of _");
    let [Pattern::Binding(binding)] = arguments.as_slice() else {
        panic!("expected a pattern binding");
    };
    assert_ne!(
        binding.identifier,
        signature.parameters[0].binding.identifier
    );
    assert_eq!(cases[1].result, Term::Local(binding.identifier));
}

#[test]
fn resolves_parameter_annotations_in_written_order() {
    let document =
        lower("FUNCTION in (element: Type) choose (value: element) RETURNS element IS value")
            .unwrap();
    let Declaration::Function { signature, body } = &document.declarations[0] else {
        panic!("expected a function");
    };
    let [element, value] = signature.parameters.as_slice() else {
        panic!("expected two parameters");
    };
    assert_eq!(*element.annotation, Term::Type);
    assert_eq!(*value.annotation, Term::Local(element.binding.identifier));
    assert_eq!(signature.result, Term::Local(element.binding.identifier));
    assert_eq!(*body, Term::Local(value.binding.identifier));

    let error =
        lower("FUNCTION choose (value: element) in (element: Type) RETURNS element IS value")
            .unwrap_err();
    assert_eq!(error.message, "unknown name `element`");
}

#[test]
fn abstracts_direct_holes_in_order_at_each_call() {
    let document = lower(
        "
FUNCTION combine (first: Type) with (second: Type) and (third: Type) RETURNS Type IS first,
FUNCTION identity of (value: Type) RETURNS Type IS value,
FUNCTION section RETURNS Type IS (combine _ with (identity of _) and _),
",
    )
    .unwrap();
    let Declaration::Function { body, .. } = &document.declarations[2] else {
        panic!("expected a function declaration");
    };
    let Term::Function {
        parameter: first,
        body,
    } = body
    else {
        panic!("expected the first hole to introduce a function");
    };
    let Term::Function {
        parameter: third,
        body,
    } = body.as_ref()
    else {
        panic!("expected the third hole to introduce a function");
    };
    let Term::Apply { arguments, .. } = body.as_ref() else {
        panic!("expected an application");
    };
    let [first_argument, second_argument, third_argument] = arguments.as_slice() else {
        panic!("expected three arguments");
    };
    assert_eq!(*first_argument, Term::Local(first.identifier));
    assert_eq!(*third_argument, Term::Local(third.identifier));
    let Term::Function { parameter, body } = second_argument else {
        panic!("expected the nested hole to introduce its own function");
    };
    let Term::Apply { arguments, .. } = body.as_ref() else {
        panic!("expected a nested application");
    };
    assert_eq!(arguments, &[Term::Local(parameter.identifier)]);
}

#[test]
fn rejects_a_bare_hole_and_a_pattern_binding_outside_its_case() {
    let error = lower("FUNCTION section RETURNS Type IS _").unwrap_err();
    assert_eq!(error.message, "a hole must be a direct argument of a call");

    let error = lower(
        "FUNCTION example of (value: Type) RETURNS Type IS (MATCH value WITH [WHEN first THEN first, WHEN second THEN first])",
    )
    .unwrap_err();
    assert_eq!(error.message, "unknown name `first`");
}
