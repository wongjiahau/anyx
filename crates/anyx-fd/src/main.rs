//! `anyx-fd --anyx-protocol` reads one request on standard input and writes
//! its response on standard output, then exits.
use std::io::{Read, Write};
use std::process::ExitCode;

const USAGE: &str = "usage: anyx-fd --anyx-protocol

Finds files by name, as fd does, as an anyx protocol tool. It reads one
request on standard input and writes the response on standard output.
In the anyx shell, bring it in with:

    USE fd FROM COMMAND \"anyx-fd\",
    fd / find \"\\\\.rs$\" with { only [file] },

Send `SCHEMA` for the guide to its methods.
";

fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    if arguments != ["--anyx-protocol"] {
        let _ = std::io::stderr().write_all(USAGE.as_bytes());
        return ExitCode::from(2);
    }
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
    let response = anyx_fd::answer(&request, &directory);
    match std::io::stdout().write_all(response.as_bytes()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(_) => ExitCode::FAILURE,
    }
}
