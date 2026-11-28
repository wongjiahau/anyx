//! A tool that is a program serving an anyx core schema:
//! `USE fd FROM COMMAND "anyx-fd"`.
//!
//! The shell asks the program for its schema once, with `--anyx-schema`,
//! and checks it. Each call is then checked against the schema, written as
//! the JSON call the program's generated server reads, and sent to the
//! program run with `--anyx-protocol` in the shell's working directory. The
//! shell writes the request to its standard input, closes it, and reads the
//! answer from its standard output, so a request needs no framing.
use std::io::Write;
use std::process::{Command, Stdio};

use anyx_core_generate::client::{Call, CallError, Client};

/// The flag a program answers a request on.
pub const FLAG: &str = "--anyx-protocol";

/// The flag a program writes its schema on.
pub const SCHEMA_FLAG: &str = "--anyx-schema";

pub struct CommandTool {
    program: String,
    client: Client,
}

impl CommandTool {
    /// Asks `program` for its schema and checks it.
    pub fn open(program: &str) -> Result<Self, String> {
        let schema = run(program, SCHEMA_FLAG, "")?;
        let client = Client::load(&schema, program)
            .map_err(|error| format!("the schema of `{program}` has errors:\n{error}"))?;
        Ok(Self {
            program: program.to_string(),
            client,
        })
    }

    /// `source`, an expression written against the schema, as a call.
    /// `Err(None)` when it names nothing the schema declares.
    pub fn call(&self, source: &str) -> Result<Call, Option<(usize, String)>> {
        self.client
            .call_written(source)
            .map_err(|error| match error {
                CallError::Elsewhere => None,
                CallError::Invalid { span, message } => Some((span.start, message)),
            })
    }

    /// Sends `call` and reads its answer as an anyx value. A program that
    /// fails, or answers something that isn't an answer to the call, gives
    /// `INTERNAL ERROR`.
    pub fn invoke(&self, call: &Call) -> Result<String, String> {
        let answer = run(&self.program, FLAG, &call.request).map_err(|_| internal_error())?;
        self.client
            .answer(call, &answer)
            .map_err(|message| format!("INVALID REQUEST {message:?}\n"))
    }
}

fn internal_error() -> String {
    "INTERNAL ERROR\n".to_owned()
}

/// Runs `program` with `flag`, writes `request`, and returns what it wrote
/// to standard output when it exits successfully.
fn run(program: &str, flag: &str, request: &str) -> Result<String, String> {
    let mut child = Command::new(program)
        .arg(flag)
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
            "`{program} {flag}` failed ({}){}{}",
            output.status,
            if stderr.trim().is_empty() { "" } else { ":\n" },
            stderr.trim_end()
        ));
    }
    String::from_utf8(output.stdout)
        .map_err(|_| format!("`{program} {flag}` answered with text that isn't UTF-8"))
}
