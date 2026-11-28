use anyx_core::parser::{Declaration, Expression, SignatureResult, parse_document};

#[test]
fn parses_core_declarations() {
    let source = "
DATA Natural : Type WITH [zero : Natural, successor of (number: Natural) : Natural],
FUNCTION predecessor of (number: Natural) : Natural IS (MATCH number WITH [WHEN zero THEN zero, WHEN successor of (previous) THEN previous]),
FUNCTION successor function : ((number: Natural) -> Natural) IS (successor of _),
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
        successor_function.typed_head.result,
        SignatureResult::Expression(Expression::FunctionType { .. })
    ));
    assert!(
        matches!(&successor_function.body, Expression::Call { arguments, .. }
        if matches!(arguments.as_slice(), [Expression::Hole(_)]))
    );
}

#[test]
fn a_function_without_a_body_is_served() {
    let source = "DATA Natural : Type WITH [],\nFUNCTION served number : Natural";
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
fn parses_integer_and_text_literals() {
    for (literal, expected) in [("12", "12"), ("-3", "-3")] {
        let source = format!("FUNCTION example : Integer IS {literal}");
        let document = parse_document(&source).document.unwrap();
        let [Declaration::Function(example)] = document.declaration.as_slice() else {
            panic!("expected one function");
        };
        assert!(matches!(&example.body, Expression::Number(number) if number.value == expected));
    }
    let document = parse_document("FUNCTION example : Text IS \"src\"")
        .document
        .unwrap();
    let [Declaration::Function(example)] = document.declaration.as_slice() else {
        panic!("expected one function");
    };
    assert!(matches!(&example.body, Expression::Text(text) if text.value.value == "src"));

    let source = "FUNCTION example : Natural IS 1.5";
    let parsed = parse_document(source);
    assert!(parsed.syntax_errors.is_empty());
    let rendered = parsed.document.unwrap_err().render(source, "sample.anyx");
    assert!(rendered.starts_with("sample.anyx:1:31:"), "{rendered}");
}

#[test]
fn requires_a_colon_between_the_binder_and_its_type() {
    for parameter in ["(number ⁽Natural)", "(number + Natural)"] {
        let source = format!("FUNCTION identity of {parameter} : Natural IS number");
        let parsed = parse_document(&source);
        assert!(parsed.syntax_errors.is_empty());
        assert!(parsed.document.is_err(), "{source}");
    }
}
