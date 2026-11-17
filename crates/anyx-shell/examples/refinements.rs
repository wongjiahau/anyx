//! The regular REPL with a local `checked` tool; no external server is needed.
#[path = "../src/repl.rs"]
mod repl;

use anyx_protocol::{Call, Outcome, Schema, Value};
use anyx_shell::{Interpreter, Tool};
use std::io::{self, Write};
use std::process::ExitCode;

struct Checked(Schema);
impl Tool for Checked {
    fn schema(&self) -> &Schema {
        &self.0
    }
    fn invoke(&mut self, call: &Call) -> Outcome {
        Outcome::Value(
            if self.0.method_at(call.method).shape.text == "broken limit _" {
                Value::Integer(30)
            } else {
                call.arguments[0].clone()
            },
        )
    }
}

fn main() -> ExitCode {
    let mut interpreter = Interpreter::new();
    let schema =
        Schema::parse(include_str!("refinements.protocol.anyx")).expect("valid demo schema");
    interpreter
        .register("checked", Checked(schema))
        .expect("unique demo tool");
    match repl::run(interpreter) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

// Shared REPL callback: print standalone values or the shell's diagnostic.
fn execute(interpreter: &mut Interpreter, source: &str) -> Result<bool, String> {
    let execution = match interpreter.execute(source) {
        Ok(execution) => execution,
        Err(problem) => {
            io::stderr()
                .write_all(anyx_shell::diagnostic(problem).as_bytes())
                .map_err(|e| e.to_string())?;
            return Ok(false);
        }
    };
    io::stdout()
        .write_all(execution.output.as_bytes())
        .map_err(|e| e.to_string())?;
    if let Some(error) = execution.error {
        io::stderr()
            .write_all(error.as_bytes())
            .map_err(|e| e.to_string())?;
        Ok(false)
    } else {
        Ok(true)
    }
}
