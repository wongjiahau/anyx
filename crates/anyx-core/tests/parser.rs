use anyx_core::parser::{Declaration, Expression, parse_document};

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
        successor_function.result,
        Expression::FunctionType { .. }
    ));
    assert!(
        matches!(&successor_function.body, Expression::Call { arguments, .. }
        if matches!(arguments.as_slice(), [Expression::Hole(_)]))
    );
}

#[test]
fn reports_a_missing_function_body_at_the_end_of_the_declaration() {
    let source = "DATA Natural RETURNS Type WITH [],\nFUNCTION missing body RETURNS Natural";
    let parsed = parse_document(source);
    assert!(
        parsed.syntax_errors.is_empty(),
        "{:?}",
        parsed.syntax_errors
    );
    let error = parsed.document.unwrap_err();
    let rendered = error.render(source, "sample.anyx");
    assert!(rendered.starts_with("sample.anyx:2:38:"), "{rendered}");
    assert!(rendered.contains("expected the word `IS`"), "{rendered}");
}
