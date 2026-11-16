//! Protocol schema checking against open buffers, also usable in WebAssembly.
use crate::{Language, Problem, ProblemKind};
use lsp_types::Url;
use std::collections::HashMap;

/// The protocol schema language. Enable the `protocol` feature and pass
/// `&Protocol` to `Server::new`, `serve`, or `wasm::LanguageServer::new`.
/// Imported files must be supplied as open buffers by the editor.
pub struct Protocol;

impl Language for Protocol {
    fn check(&self, source: &str) -> Vec<Problem> {
        anyx_protocol::Schema::parse(source)
            .err()
            .unwrap_or_default()
            .into_iter()
            .map(problem)
            .collect()
    }

    fn check_workspace(
        &self,
        documents: &HashMap<Url, String>,
    ) -> Option<HashMap<Url, Vec<Problem>>> {
        let mut problems: HashMap<Url, Vec<Problem>> = HashMap::new();
        for entry in documents.keys() {
            let checked = anyx_protocol::Schema::load(&document_path(entry), |path| {
                let uri = document_uri(entry, path);
                documents
                    .get(&uri)
                    .cloned()
                    .ok_or_else(|| format!("open the imported file `{path}` in the editor"))
            });
            if let Err(diagnostics) = checked {
                for diagnostic in diagnostics {
                    let uri = document_uri(entry, &diagnostic.path);
                    let found = problems.entry(uri).or_default();
                    let problem = problem(diagnostic);
                    if !found.contains(&problem) {
                        found.push(problem);
                    }
                }
            }
        }
        for found in problems.values_mut() {
            found.sort_by(|left, right| {
                (left.span.start, &left.message).cmp(&(right.span.start, &right.message))
            });
        }
        Some(problems)
    }

    fn format(&self, source: &str) -> Option<String> {
        anyx_protocol::format_schema(source).ok()
    }
}

fn problem(diagnostic: anyx_protocol::Diagnostic) -> Problem {
    Problem {
        span: diagnostic.span,
        message: diagnostic.message,
        expected: Vec::new(),
        kind: ProblemKind::Grammar,
    }
}

// Decode file URLs before passing paths to the platform-independent loader.
fn document_path(uri: &Url) -> String {
    percent_encoding::percent_decode_str(uri.path())
        .decode_utf8_lossy()
        .into_owned()
}

fn document_uri(entry: &Url, path: &str) -> Url {
    let mut uri = entry.clone();
    uri.set_path(path);
    uri.set_query(None);
    uri.set_fragment(None);
    uri
}
