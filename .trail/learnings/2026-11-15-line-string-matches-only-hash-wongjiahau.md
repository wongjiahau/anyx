---
title: "LINE STRING matches only # line strings; a string an optional part refused is not another call shape"
author: Wongjiahau
date: 2026-11-15
branch: work/line-string
id: l-5c1e7a94
scope: repo
status: active
src: agent
touches:
  - crates/anyx/src/string.rs
  - crates/anyx/src/lexer.rs
  - crates/anyx-generator/grammar.anyx
  - crates/anyx-generator/src/lower.rs
  - crates/anyx-generator/src/model.rs
  - crates/anyx-generator/src/generate.rs
  - crates/anyx-generator/src/overlap.rs
  - crates/anyx-generator/src/support.rs
  - crates/anyx-protocol/protocol.anyx
---

## Decided
- `StringLiteral` keeps its `kind` (`StringKind::Line`, `Quoted`, `Raw`), set where the lexer builds each literal. The formatter compares values only, so it is unchanged.
- `LINE STRING` is its own rule in grammar.anyx (`line string`), with three forms, `LINE STRING`, `LINE STRING "x"` and `LINE STRING [conditions]` (the calls `LINE STRING` and `LINE STRING _`), since all-caps words side by side join into one call name. `LINE` alone stays free as a word of a user's language (`WORD "LINE"` in the corners fixture).
- The model keeps one `Pattern::String(StringPattern { line_only, conditions })`, so `LINE STRING` decodes, describes and overlaps like `STRING`. Overlap ignores the kind: every line string is a string, so only conditions can separate them.
- `support::line_string` names a wrong kind in `found`: ``found the quoted string "x"; expected a `#` line string``.
- The protocol's docs (protocol header, record, field, choice, variant, TYPE, ERRORS group, method; an error is a variant) are `LINE STRING`.

## Watch out
- A quoted doc first broke the call's shape check: `optional` refused the string, then `word("METHOD")` met a node and `other_call` replaced the refusal with "found the call `_ METHOD _`; expected the call `_? METHOD …`" (72 ways). `Sequence::may_be_other_call` now keeps the refusal when the item at the front is a string the optional part refused at the same span. It is narrow on purpose: for any refused node, a call that is really another shape would lose its better error. The corners fixture's `noted` choice (`"A doc." NOTED 1`) pins this down.
- Inside a `ONE OF`, the first choice's `found` wins a merge at the same span, so a quoted string missing both `STRING "exact"` and `LINE STRING "exact line"` says "the string", not "the quoted string".
- Changing `support.rs` changes every generated file, including anyx-protocol's `src/grammar.rs` (about 9 minutes to regenerate). Regenerate the generator's own decoder with the old lowering first when grammar.anyx gains a rule the lowering uses.

In the code: `StringKind` in crates/anyx/src/string.rs; `line string` in crates/anyx-generator/grammar.anyx; `StringPattern` in src/model.rs; `support::line_string` and `Sequence::may_be_other_call` in src/support.rs; tests in tests/check.rs (`choices_that_compete_get_a_warning`), tests/generated.rs (`a_line_string_decodes_like_any_string`), crates/anyx-protocol/tests/schema.rs (`a_doc_is_a_line_string`).
