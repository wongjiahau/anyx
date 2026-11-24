//! Syntax, name resolution and term representation for the dependently typed anyx core.

pub mod kernel;
pub mod lowering;
// Regenerate from the workspace root with:
// cargo run -p anyx-generator --bin anyx-generate -- core/core.grammar.anyx > crates/anyx-core/src/parser.rs
pub mod parser;
