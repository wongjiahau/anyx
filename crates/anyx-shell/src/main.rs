mod repl;

use std::io::{self, IsTerminal, Read, Write};
use std::process::ExitCode;

use anyx_protocol::Problem;
use anyx_shell::{Interpreter, boolean::Boolean, diagnostic, environment::Environment};

const USAGE: &str = "anyx-shell [--interactive | --expression SOURCE | FILE | -]\nWith no arguments, start a REPL on a terminal or read standard input when piped.\n--interactive starts a REPL explicitly; - always reads a batch document.\nCompletion opens while typing; Ctrl-Space opens it explicitly. Arrow keys choose; Enter accepts.\nTab and Shift-Tab move between snippet arguments; typing replaces the selected argument.\nEsc dismisses completion or ends snippet editing. History lasts for the session.\nCtrl-C cancels input; Ctrl-D exits. Open brackets and strings continue on the next line.\nCalls use scope / method arguments. Successful results are anyx values.\nExample: anyx-shell --expression 'environment / list variables'";

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
    let interactive =
        (arguments.is_empty() && io::stdin().is_terminal() && io::stdout().is_terminal())
            || (arguments.len() == 1 && arguments[0] == "--interactive");
    if interactive {
        return repl::run(interpreter()?);
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
    execute(&mut interpreter()?, &source)
}

fn interpreter() -> Result<Interpreter, String> {
    let mut interpreter = Interpreter::new();
    interpreter.register("environment", Environment::from_process()?)?;
    interpreter.register("boolean", Boolean)?;
    Ok(interpreter)
}

fn execute(interpreter: &mut Interpreter, source: &str) -> Result<bool, String> {
    let execution = match interpreter.execute(source) {
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
