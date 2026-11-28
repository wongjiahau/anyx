use anyx::{Span, line_column};
use anyx_core::{
    kernel::{self, Term, TypeChecker},
    lowering,
    parser::{self, support},
};
use std::{env, fmt::Display, fs, process::ExitCode};

const USAGE: &str = "Usage: anyx-core FILE [--evaluate EXPRESSION --type TYPE]";

fn main() -> ExitCode {
    let arguments: Vec<_> = env::args().skip(1).collect();
    if arguments == ["--help"] {
        println!("{USAGE}");
        return ExitCode::SUCCESS;
    }
    match run(&arguments) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}

fn run(arguments: &[String]) -> Result<(), String> {
    let (path, evaluation) = match arguments {
        [path] => (path, None),
        [path, evaluate, expression, annotation, expected]
            if evaluate == "--evaluate" && annotation == "--type" =>
        {
            (path, Some((expression, expected)))
        }
        _ => return Err(USAGE.to_owned()),
    };
    let source = fs::read_to_string(path).map_err(|error| format!("{path}: {error}"))?;
    let syntax = decode(parser::parse_document(&source), &source, path)?;
    let lowered = lowering::lower_document(&syntax)
        .map_err(|error| render(&source, path, error.span, &error))?;
    let document = kernel::Document::try_from(&lowered)
        .map_err(|error| render(&source, path, syntax.span, error))?;
    let checker = TypeChecker::default()
        .check_document(&document)
        .map_err(|error| render(&source, path, error.span(&syntax), &error))?;

    if let Some((expression, expected)) = evaluation {
        let (expected_term, expected_span) = expression_term(&syntax, expected, "--type")?;
        checker
            .check_type(&expected_term)
            .map_err(|error| render(expected, "--type", expected_span, error))?;
        let (term, span) = expression_term(&syntax, expression, "--evaluate")?;
        checker
            .check(&term, &expected_term)
            .map_err(|error| render(expression, "--evaluate", span, error))?;
        let normal = checker
            .normalize(&term)
            .map_err(|error| render(expression, "--evaluate", span, error))?;
        println!("{normal:#?}");
    }
    Ok(())
}

fn expression_term(
    document: &parser::Document,
    source: &str,
    name: &str,
) -> Result<(Term, Span), String> {
    let parsed = support::parse(source, |nodes| {
        let mut expressions = support::document(nodes);
        let expression = expressions.one(parser::decode_expression, "an expression")?;
        expressions.finish()?;
        Ok(expression)
    });
    let expression = decode(parsed, source, name)?;
    let span = expression.span();
    let lowered = lowering::lower_expression(document, &expression)
        .map_err(|error| render(source, name, error.span, &error))?;
    let term = Term::try_from(&lowered).map_err(|error| render(source, name, span, error))?;
    Ok((term, span))
}

fn decode<T>(parsed: support::Parsed<T>, source: &str, name: &str) -> Result<T, String> {
    if !parsed.syntax_errors.is_empty() {
        return Err(parsed
            .syntax_errors
            .iter()
            .map(|error| error.render(source, name))
            .collect::<Vec<_>>()
            .join("\n"));
    }
    parsed.document.map_err(|error| error.render(source, name))
}

fn render(source: &str, name: &str, span: Span, error: impl Display) -> String {
    let position = line_column(source, span.start);
    let line = source.lines().nth(position.line - 1).unwrap_or("");
    let width = source
        .get(span.start..span.end)
        .and_then(|text| text.lines().next())
        .map_or(1, |text| text.chars().count().max(1));
    format!(
        "{name}:{position}: {error}\n  {line}\n  {}{}",
        " ".repeat(position.column - 1),
        "^".repeat(width)
    )
}
