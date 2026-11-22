//! Schema names as Rust and TypeScript names.

use std::borrow::Cow;
use std::fmt::Write;

const RUST_KEYWORDS: [&str; 51] = [
    "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum", "extern",
    "false", "fn", "for", "gen", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut",
    "pub", "ref", "return", "self", "Self", "static", "struct", "super", "trait", "true", "type",
    "unsafe", "use", "where", "while", "abstract", "become", "box", "do", "final", "macro",
    "override", "priv", "try", "typeof", "unsized", "virtual",
];

fn words(name: &str) -> impl Iterator<Item = Cow<'_, str>> {
    name.split(' ').filter(|word| *word != "_").map(|word| {
        if word.starts_with(|c: char| c.is_ascii_alphabetic()) {
            return Cow::Borrowed(word);
        }
        // Symbol words have no identifier spelling. Encode their UTF-8 bytes
        // in hex so different operators produce different names.
        let mut encoded = String::from("symbol");
        for byte in word.bytes() {
            write!(encoded, "{byte:02x}").unwrap();
        }
        Cow::Owned(encoded)
    })
}

fn capitalized(word: &str) -> String {
    let mut chars = word.chars();
    chars
        .next()
        .map(|first| first.to_ascii_uppercase().to_string() + chars.as_str())
        .unwrap_or_default()
}

/// `file summary` as `FileSummary`.
pub fn pascal_case(name: &str) -> String {
    words(name).map(|word| capitalized(&word)).collect()
}

/// `new revision` as `new_revision`, escaped when it is a keyword.
pub fn snake_case(name: &str) -> String {
    let joined = words(name).collect::<Vec<_>>().join("_");
    match RUST_KEYWORDS.contains(&joined.as_str()) {
        true => format!("r#{joined}"),
        false => joined,
    }
}

/// `new revision` as `newRevision`, as the JSON names it.
pub fn camel_case(name: &str) -> String {
    words(name)
        .enumerate()
        .map(|(index, word)| match index {
            0 => word.into_owned(),
            _ => capitalized(&word),
        })
        .collect()
}

/// `text` as a Rust raw string literal.
pub fn raw_string(text: &str) -> String {
    let hashes = (0..)
        .find(|count| !text.contains(&format!("\"{}", "#".repeat(*count))))
        .unwrap_or(0)
        .max(1);
    let hashes = "#".repeat(hashes);
    format!("r{hashes}\"{text}\"{hashes}")
}

/// A doc comment of `doc`, each line after `prefix`.
pub fn doc_comment(doc: &Option<String>, prefix: &str) -> String {
    doc.as_deref()
        .map(|doc| {
            doc.lines()
                .map(|line| match line.is_empty() {
                    true => format!("{prefix}\n"),
                    false => format!("{prefix} {line}\n"),
                })
                .collect()
        })
        .unwrap_or_default()
}
