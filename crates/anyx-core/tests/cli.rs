use std::process::{Command, Output};

fn run_sample(arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_anyx-core"))
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/sample.anyx"
        ))
        .args(arguments)
        .output()
        .unwrap()
}

#[test]
fn checks_and_evaluates_the_sample_declarations() {
    let checked = run_sample(&[]);
    assert!(
        checked.status.success(),
        "{}",
        String::from_utf8_lossy(&checked.stderr)
    );
    assert!(checked.stdout.is_empty());

    let evaluated = run_sample(&[
        "--evaluate",
        "(successor of (zero)) plus (zero)",
        "--type",
        "Natural",
    ]);
    assert!(
        evaluated.status.success(),
        "{}",
        String::from_utf8_lossy(&evaluated.stderr)
    );
    assert_successor_of_zero(&evaluated.stdout);
}

fn assert_successor_of_zero(output: &[u8]) {
    // A closed `Natural` normalizes to a number.
    let normal_form = String::from_utf8_lossy(output);
    assert_eq!(
        normal_form.split_whitespace().collect::<String>(),
        "Natural(1,)"
    );
}

#[test]
fn reports_expression_and_annotation_errors_at_their_source() {
    for (expression, annotation, expected) in [
        (
            "missing",
            "Natural",
            "--evaluate:1:1: unknown name `missing`",
        ),
        (
            "Type",
            "Natural",
            "--evaluate:1:1: expression does not have the expected type",
        ),
        (
            "zero",
            "zero",
            "--type:1:1: expression does not have the expected type",
        ),
    ] {
        let output = run_sample(&["--evaluate", expression, "--type", annotation]);
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        let diagnostic = String::from_utf8_lossy(&output.stderr);
        assert!(diagnostic.starts_with(expected), "{diagnostic}");
        let source = if expected.starts_with("--type") {
            annotation
        } else {
            expression
        };
        assert!(
            diagnostic.ends_with(&format!("\n  {source}\n  {}\n", "^".repeat(source.len()))),
            "{diagnostic}"
        );
    }
}

#[test]
fn evaluates_a_type_with_an_explicit_type_annotation() {
    let output = run_sample(&["--evaluate", "Natural", "--type", "Type"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("name: \"Natural\""));
}
