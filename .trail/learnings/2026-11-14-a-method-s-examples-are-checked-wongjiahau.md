---
title: "A method's examples are checked against the schema, and their docs are read from inside the call"
author: Wongjiahau
date: 2026-11-14
branch: work/named-types-and-examples
cost: { minutes: 30, tokens: 121102 }
id: l-011798d1
scope: repo
status: active
src: agent
touches:
  - crates/anyx-protocol-generate/README.md
  - crates/anyx-protocol-generate/src/lib.rs
  - crates/anyx-protocol-generate/src/rust.rs
  - crates/anyx-protocol-generate/src/typescript.rs
  - crates/anyx-protocol-generate/tests/fixtures/named.anyx
  - crates/anyx-protocol-generate/tests/generated.rs
  - crates/anyx-protocol-generate/tests/generated/named.rs
  - crates/anyx-protocol-generate/tests/generated/named.ts
  - crates/anyx-protocol-generate/tests/named_round_trip.rs
  - crates/anyx-protocol/PROTOCOL.md
---

## Decided

- `EXAMPLES [ … ]` is a method clause, last, after `ERRORS`. Each item is a call of the method by its full name (scope path included, as a request writes it) and its outcome as a response line writes it: `RETURNS value`, `RETURNS NOTHING` or `ERROR error`. Methods only; not on RECORD or CHOICE.
- The checker decodes each call against its own method (an example of another method is an error) and its outcome against RETURNS/ERRORS, reusing the request and response decoders, so the diagnostics are the same ones a client would get, prefixed "in an example of `…`".
- Examples are checked after the `DEFAULT`s are read (so a returned record left without a defaulted field gets its default), and only once the rest of the schema has no diagnostics: they need the finished Schema, and a mistake elsewhere would only repeat in them.
- `EXAMPLES []` parses (ZERO OR MORE) so that the checker can say "found `EXAMPLES` without an example"; with ONE OR MORE the generated decoder fell back to another method form and reported a misleading `RETURNS` error.
- The guide's sample call is the first example of the widest method that has examples; `sample_call` is used only when no method has any.
- Generated Rust and TypeScript put the examples at the end of the method's doc comment, in an `anyx` code block.

## Tried and ruled out

- One method form with optional clauses: the grammar language has no optional words, so the method has 8 forms (value/nothing × errors and examples / errors / examples / neither).

## Watch out

- A `#` doc before an example does not land on the example: `RETURNS`/`ERROR` split the run first, so the doc is the first argument of the call's leftmost `_ …` call (for `files/read "a"`, inside `_ files`). `without_doc` in the checker digs it out recursively; it is recognised as a line string by the source at its span starting with `#`.
- `format_schema` breaks before `EXAMPLES` only when the method doesn't fit in 100 columns, like `ERRORS`; the list itself breaks one example per line when it holds a `#` doc or is too long. A short `EXAMPLES [ping RETURNS NOTHING]` stays on its line.
- The LSP does not know schemas yet (anyx-lsp has no anyx-protocol dependency), so hovering a method doesn't show its examples.

In the code:
- crates/anyx-protocol/protocol.anyx: `method` (8 forms), `examples`, `example`.
- crates/anyx-protocol/src/check.rs: `method_parts`, `PendingExamples`, `Checker::examples`, `Checker::without_doc`.
- crates/anyx-protocol/src/codec.rs `decode_call_of`, src/response.rs `decode_ending`.
- crates/anyx-protocol/src/schema.rs `Example { doc, text, call, arguments, outcome }`.
- crates/anyx-protocol-generate/src/lib.rs `examples_doc`.
- Tests: crates/anyx-protocol/tests/examples.rs.
