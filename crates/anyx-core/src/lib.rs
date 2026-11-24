//! Syntax and name resolution for the dependently typed anyx core language.

pub mod lowering;
// Regenerate from the workspace root with:
// cargo run -p anyx-generator --bin anyx-generate -- core/core.grammar.anyx > crates/anyx-core/src/parser.rs
pub mod parser;
