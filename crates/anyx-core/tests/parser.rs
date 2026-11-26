use anyx_core::parser::{Declaration, Expression, SignatureResult, parse_document};

#[test]
fn parses_core_declarations() {
    let source = "
DATA Natural RETURNS Type WITH [zero RETURNS Natural, successor of (number: Natural) RETURNS Natural],
FUNCTION predecessor of (number: Natural) RETURNS Natural IS (MATCH number WITH [WHEN zero THEN zero, WHEN successor of (previous) THEN previous]),
FUNCTION successor function RETURNS ((number: Natural) RETURNS Natural) IS (successor of _),
";
    let parsed = parse_document(source);
    assert!(
        parsed.syntax_errors.is_empty(),
        "{:?}",
        parsed.syntax_errors
    );
    let document = parsed.document.unwrap();
    let [
        Declaration::Data(natural),
        Declaration::Function(predecessor),
        Declaration::Function(successor_function),
    ] = document.declaration.as_slice()
    else {
        panic!("expected Natural and two function declarations");
    };
    assert_eq!(natural.constructors.value.len(), 2);
    assert!(matches!(&predecessor.body, Expression::Match { cases, .. } if cases.value.len() == 2));
    assert!(matches!(
        successor_function.signature_result,
        SignatureResult::Expression(Expression::FunctionType { .. })
    ));
    assert!(
        matches!(&successor_function.body, Expression::Call { arguments, .. }
        if matches!(arguments.as_slice(), [Expression::Hole(_)]))
    );
}

#[test]
fn a_function_without_a_body_is_served() {
    let source = "DATA Natural RETURNS Type WITH [],\nFUNCTION served number RETURNS Natural";
    let parsed = parse_document(source);
    assert!(
        parsed.syntax_errors.is_empty(),
        "{:?}",
        parsed.syntax_errors
    );
    let document = parsed.document.unwrap();
    assert!(matches!(
        document.declaration.as_slice(),
        [_, anyx_core::parser::Declaration::ServedFunction(_)]
    ));
}

#[test]
fn rejects_number_and_string_literals() {
    for literal in ["1", "\"zero\""] {
        let source = format!("FUNCTION example RETURNS Natural IS {literal}");
        let parsed = parse_document(&source);
        assert!(parsed.syntax_errors.is_empty());
        let error = parsed.document.unwrap_err();
        let rendered = error.render(&source, "sample.anyx");
        assert!(rendered.starts_with("sample.anyx:1:37:"), "{rendered}");
    }
}

#[test]
fn requires_a_colon_between_the_binder_and_its_type() {
    for parameter in ["(number ⁽Natural)", "(number + Natural)"] {
        let source = format!("FUNCTION identity of {parameter} RETURNS Natural IS number");
        let parsed = parse_document(&source);
        assert!(parsed.syntax_errors.is_empty());
        assert!(parsed.document.is_err(), "{source}");
    }
}
