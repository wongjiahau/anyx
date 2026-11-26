//! `anyx-core-generate SCHEMA (--rust | --typescript)`: print the generated
//! Rust module or TypeScript client of a core schema.

use std::process::ExitCode;

fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let [path, target] = arguments.as_slice() else {
        eprintln!("Usage: anyx-core-generate SCHEMA (--rust | --typescript)");
        return ExitCode::FAILURE;
    };
    let result = std::fs::read_to_string(path)
        .map_err(|error| format!("{path}: {error}"))
        .and_then(|source| anyx_core_generate::load(&source, path).map_err(|error| error.message))
        .and_then(|schema| {
            match target.as_str() {
                "--rust" => anyx_core_generate::rust(&schema),
                "--typescript" => anyx_core_generate::typescript(&schema),
                _ => return Err(format!("unknown target `{target}`")),
            }
            .map_err(|error| error.message)
        });
    match result {
        Ok(code) => {
            print!("{code}");
            ExitCode::SUCCESS
        }
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}
