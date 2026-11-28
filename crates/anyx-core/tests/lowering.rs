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
FUNCTION predecessor of (number: Natural) : Natural IS (MATCH number WITH [WHEN zero THEN zero, WHEN successor of (number) THEN number]),
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
            candidates: vec!["zero".to_owned()],
            id: None,
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
    let Pattern::Constructor {
        name, arguments, ..
    } = &cases[1].pattern
    else {
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
        lower("FUNCTION in (element: Type) choose (value: element) : element IS value").unwrap();
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

    let error = lower("FUNCTION choose (value: element) in (element: Type) : element IS value")
        .unwrap_err();
    assert_eq!(error.message, "unknown name `element`");
}

#[test]
fn abstracts_direct_holes_in_order_at_each_call() {
    let document = lower(
        "
FUNCTION combine (first: Type) with (second: Type) and (third: Type) : Type IS first,
FUNCTION identity of (value: Type) : Type IS value,
FUNCTION section : Type IS (combine _ with (identity of _) and _),
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
    let error = lower("FUNCTION section : Type IS _").unwrap_err();
    assert_eq!(error.message, "a hole must be a direct argument of a call");

    let error = lower(
        "FUNCTION example of (value: Type) : Type IS (MATCH value WITH [WHEN first THEN first, WHEN second THEN first])",
    )
    .unwrap_err();
    assert_eq!(error.message, "unknown name `first`");
}

#[test]
fn rejects_invalid_names_at_the_offending_token() {
    for (source, token, message) in [
        (
            "FUNCTION example : Type IS Type, FUNCTION example : Type IS Type",
            "example",
            "duplicate declaration of `example`",
        ),
        (
            "FUNCTION choose (value: Type) and (value: Type) : Type IS Type",
            "value",
            "duplicate binding of `value`",
        ),
        ("FUNCTION Type : Type IS Type", "Type", "`Type` is reserved"),
        (
            "FUNCTION example : Type IS (MATCH Type WITH [WHEN missing (field) THEN field])",
            "missing",
            "unknown constructor `missing _`",
        ),
    ] {
        let error = lower(source).unwrap_err();
        assert_eq!(error.message, message);
        let start = if message.starts_with("duplicate") {
            source.rfind(token).unwrap()
        } else {
            source.find(token).unwrap()
        };
        assert_eq!(error.span.start, start);
        assert_eq!(&source[error.span.start..error.span.end], token);
    }
}

#[test]
fn rejects_kind_outside_a_declaration_result() {
    for source in [
        "FUNCTION example (item: Kind) : Type IS item",
        "FUNCTION example : Kind IS Kind",
        "FUNCTION example : ((item: Type) -> Kind) IS Type",
        "DATA Example : Kind WITH [example : Kind]",
    ] {
        let error = lower(source).unwrap_err();
        assert_eq!(&source[error.span.start..error.span.end], "Kind");
        assert_eq!(
            error.message,
            "`Kind` is only allowed as a declaration result"
        );
    }
}
