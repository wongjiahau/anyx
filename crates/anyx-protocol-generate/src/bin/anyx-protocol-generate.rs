//! `anyx-protocol-generate <rust|wasm|typescript> <schema>` writes the code of
//! a protocol to standard output, following explicit imports from its entry file.

use anyx_protocol::Schema;
use std::process::ExitCode;

fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let [target, path] = arguments.as_slice() else {
        eprintln!("usage: anyx-protocol-generate <rust|wasm|typescript> <entry.protocol.anyx>");
        return ExitCode::FAILURE;
    };
    if !is_protocol_file(path) {
        eprintln!("{path}: expected a protocol definition file ending in `.protocol.anyx`");
        return ExitCode::FAILURE;
    }
    let mut sources = std::collections::BTreeMap::new();
    let schema = match Schema::load(path, |file| {
        if !is_protocol_file(file) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "expected an imported protocol definition file ending in `.protocol.anyx`",
            ));
        }
        let source = std::fs::read_to_string(file)?;
        sources.insert(file.to_string(), source.clone());
        Ok::<_, std::io::Error>(source)
    }) {
        Ok(schema) if schema.is_protocol() => schema,
        Ok(_) => {
            eprintln!(
                "{path}: a library has no endpoint; expected an entry with a `PROTOCOL` header"
            );
            return ExitCode::FAILURE;
        }
        Err(diagnostics) => {
            for diagnostic in diagnostics {
                let source = sources.get(&diagnostic.path).map_or("", String::as_str);
                eprintln!("{}", diagnostic.render(source, &diagnostic.path));
            }
            return ExitCode::FAILURE;
        }
    };
    let code = match target.as_str() {
        "rust" => anyx_protocol_generate::rust(&schema).map_err(|error| error.to_string()),
        "wasm" => Ok(anyx_protocol_generate::wasm(&schema)),
        "typescript" => {
            anyx_protocol_generate::typescript(&schema).map_err(|error| error.to_string())
        }
        other => Err(format!(
            "found the target `{other}`; expected `rust`, `wasm` or `typescript`"
        )),
    };
    let code = code.and_then(|code| match target.as_str() {
        "typescript" => Ok(code),
        _ => anyx_protocol_generate::format_rust(&code).map_err(|error| error.to_string()),
    });
    match code {
        Ok(code) => {
            print!("{code}");
            ExitCode::SUCCESS
        }
        Err(message) => {
            eprintln!("{path}: {message}");
            ExitCode::FAILURE
        }
    }
}

fn is_protocol_file(path: &str) -> bool {
    std::path::Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.ends_with(".protocol.anyx"))
}
