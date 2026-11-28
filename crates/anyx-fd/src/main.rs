//! `anyx-fd --anyx-protocol` reads one request on standard input and writes
//! its answer on standard output, then exits. `anyx-fd --anyx-schema` writes
//! its schema.
use std::io::{Read, Write};
use std::process::ExitCode;

const USAGE: &str = "usage: anyx-fd (--anyx-protocol | --anyx-schema)

Finds files by name, as fd does, as an anyx core tool.

  --anyx-protocol  reads a request, a JSON list of calls, on standard input
                   and writes their answers on standard output.
  --anyx-schema    writes the schema the calls are checked against.

In the anyx shell, bring it in with:

    USE fd FROM COMMAND \"anyx-fd\",
    fd / find \"\\\\.rs$\" with { only [file] },
";

fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    match arguments.as_slice() {
        [flag] if flag == "--anyx-schema" => write(anyx_fd::SCHEMA),
        [flag] if flag == "--anyx-protocol" => {
            let mut request = String::new();
            if let Err(error) = std::io::stdin().read_to_string(&mut request) {
                let _ = writeln!(
                    std::io::stderr(),
                    "anyx-fd: couldn't read the request: {error}"
                );
                return ExitCode::FAILURE;
            }
            let directory = match std::env::current_dir() {
                Ok(directory) => directory,
                Err(error) => {
                    let _ = writeln!(
                        std::io::stderr(),
                        "anyx-fd: couldn't read the working directory: {error}"
                    );
                    return ExitCode::FAILURE;
                }
            };
            write(&anyx_fd::answer(&request, &directory))
        }
        _ => {
            let _ = std::io::stderr().write_all(USAGE.as_bytes());
            ExitCode::from(2)
        }
    }
}

fn write(text: &str) -> ExitCode {
    match std::io::stdout().write_all(text.as_bytes()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(_) => ExitCode::FAILURE,
    }
}
