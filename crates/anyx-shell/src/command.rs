//! A tool that is a program: `USE fd FROM COMMAND "anyx-fd"`.
//!
//! The program is run once for each request, with `--anyx-protocol`, in the
//! shell's working directory. The shell writes the request to its standard
//! input, closes it, and reads the response from its standard output, so a
//! request needs no framing. The schema comes from the built-in
//! `SCHEMA FILES` call, which the shell checks and keeps for the session.
use std::io::Write;
use std::process::{Command, Stdio};

use anyx_protocol::{Call, Outcome, Response, Schema, Value};

use crate::Tool;

/// The flag a program answers a request on.
pub const FLAG: &str = "--anyx-protocol";

pub struct CommandTool {
    program: String,
    schema: Schema,
}

impl CommandTool {
    /// Asks `program` for its schema and checks it.
    pub fn open(program: &str) -> Result<Self, String> {
        // Every schema has the built-in calls, so any one reads the answer.
        let any = crate::environment::schema();
        let call = Call {
            method: any.schema_files(),
            arguments: Vec::new(),
        };
        let request = anyx_protocol::encode_request(any, std::slice::from_ref(&call))
            .expect("`SCHEMA FILES` has no arguments to encode");
        let answer = run(program, &request)?;
        let files = match anyx_protocol::decode_response(any, &[call.method], &answer) {
            Ok(Response::Outcomes(outcomes)) => match outcomes.as_slice() {
                [Outcome::Value(Value::List(files))] => {
                    files.iter().map(schema_file).collect::<Option<Vec<_>>>()
                }
                _ => None,
            },
            _ => None,
        }
        .filter(|files| !files.is_empty())
        .ok_or_else(|| format!("`{program} {FLAG}` didn't answer `SCHEMA FILES` with its files"))?;
        let schema = Schema::load(&files[0].0, |path| {
            files
                .iter()
                .find(|(file, _)| file == path)
                .map(|(_, source)| source.clone())
                .ok_or_else(|| format!("`{program}` didn't send `{path}`"))
        })
        .map_err(|diagnostics| {
            let rendered = diagnostics
                .iter()
                .map(|diagnostic| {
                    let source = files
                        .iter()
                        .find(|(path, _)| *path == diagnostic.path)
                        .map_or("", |(_, source)| source.as_str());
                    diagnostic.render(source, &diagnostic.path)
                })
                .collect::<Vec<_>>()
                .join("\n");
            format!("the schema of `{program}` has errors:\n{rendered}")
        })?;
        Ok(Self {
            program: program.to_string(),
            schema,
        })
    }
}

impl Tool for CommandTool {
    fn schema(&self) -> &Schema {
        &self.schema
    }

    /// A program that fails, is killed, or answers something that isn't a
    /// response to the call, gives `INTERNAL ERROR`.
    fn invoke(&mut self, call: &Call) -> Outcome {
        let Ok(request) = anyx_protocol::encode_request(&self.schema, std::slice::from_ref(call))
        else {
            return Outcome::InternalError;
        };
        let Ok(answer) = run(&self.program, &request) else {
            return Outcome::InternalError;
        };
        match anyx_protocol::decode_response(&self.schema, &[call.method], &answer) {
            Ok(Response::Outcomes(mut outcomes)) if outcomes.len() == 1 => outcomes.remove(0),
            _ => Outcome::InternalError,
        }
    }
}

fn schema_file(value: &Value) -> Option<(String, String)> {
    let Value::Record(fields) = value else {
        return None;
    };
    match (fields.get("path")?, fields.get("source")?) {
        (Value::Text(path), Value::Text(source)) => Some((path.clone(), source.clone())),
        _ => None,
    }
}

/// Runs `program` with the flag, writes `request`, and returns what it wrote
/// to standard output when it exits successfully.
fn run(program: &str, request: &str) -> Result<String, String> {
    let mut child = Command::new(program)
        .arg(FLAG)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("couldn't run `{program}`: {error}"))?;
    let mut stdin = child.stdin.take().expect("stdin is piped");
    let request = request.to_string();
    // Write on another thread, so a program that answers before it has read
    // the whole request can't block on a full pipe.
    let writer = std::thread::spawn(move || stdin.write_all(request.as_bytes()));
    let output = child
        .wait_with_output()
        .map_err(|error| format!("couldn't run `{program}`: {error}"))?;
    let _ = writer.join();
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "`{program} {FLAG}` failed ({}){}{}",
            output.status,
            if stderr.trim().is_empty() { "" } else { ":\n" },
            stderr.trim_end()
        ));
    }
    String::from_utf8(output.stdout)
        .map_err(|_| format!("`{program} {FLAG}` answered with text that isn't UTF-8"))
}
