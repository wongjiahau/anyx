use std::io::{self, Read, Write};
use std::process::ExitCode;

use anyx_protocol::Problem;
use anyx_shell::{Interpreter, diagnostic, environment::Environment};

const USAGE: &str = "anyx-shell [--expression SOURCE | FILE | -]\nWith no arguments or -, read an anyx document from standard input.\nCalls use scope / method arguments. Successful results are anyx values.\nExample: anyx-shell --expression 'environment / list variables'";

fn main() -> ExitCode {
    match run() {
        Ok(success) => {
            if success {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
        Err(message) => {
            let _ = io::stderr().write_all(
                diagnostic(Problem {
                    line: 1,
                    column: 1,
                    message,
                })
                .as_bytes(),
            );
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<bool, String> {
    let arguments: Vec<_> = std::env::args_os().skip(1).collect();
    if arguments.len() == 1 && arguments[0] == "--help" {
        let help = anyx_protocol::encode_value(
            anyx_shell::environment::schema(),
            &anyx_protocol::schema::Type::Text,
            &anyx_protocol::Value::Text(USAGE.into()),
        )
        .map_err(|error| error.to_string())?;
        io::stdout()
            .write_all(help.as_bytes())
            .map_err(|error| error.to_string())?;
        return Ok(true);
    }
    let source = match arguments.as_slice() {
        [] => read_standard_input()?,
        [path] if path == "-" => read_standard_input()?,
        [flag, source] if flag == "--expression" => source
            .to_str()
            .ok_or("the expression must be Unicode")?
            .to_string(),
        [path] if !path.to_string_lossy().starts_with('-') => std::fs::read_to_string(path)
            .map_err(|error| format!("cannot read input file: {error}"))?,
        _ => return Err(USAGE.into()),
    };
    let mut interpreter = Interpreter::new();
    interpreter.register("environment", Environment::from_process()?)?;
    let execution = match interpreter.execute(&source) {
        Ok(execution) => execution,
        Err(problem) => {
            io::stderr()
                .write_all(diagnostic(problem).as_bytes())
                .map_err(|error| error.to_string())?;
            return Ok(false);
        }
    };
    io::stdout()
        .write_all(execution.output.as_bytes())
        .map_err(|error| error.to_string())?;
    if let Some(error) = execution.error {
        io::stderr()
            .write_all(error.as_bytes())
            .map_err(|error| error.to_string())?;
        Ok(false)
    } else {
        Ok(true)
    }
}

fn read_standard_input() -> Result<String, String> {
    let mut source = String::new();
    io::stdin()
        .read_to_string(&mut source)
        .map_err(|error| format!("cannot read standard input: {error}"))?;
    Ok(source)
}
