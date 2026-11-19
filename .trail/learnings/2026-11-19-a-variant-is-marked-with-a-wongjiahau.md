---
title: "A variant is marked with a backtick, and predicates are read with grammar rules"
author: Wongjiahau
date: 2026-11-19
branch: feat/variant-backtick
cost: { minutes: 1, tokens: 2171 }
id: l-dff6a5dc
scope: repo
status: active
src: agent
touches:
  - crates/anyx-protocol/protocol.grammar.anyx
  - crates/anyx-protocol/src/refinement.rs
  - crates/anyx/src/format.rs
  - crates/anyx-protocol-generate/src/typescript_native.ts
  - crates/anyx-shell/src/compile.rs
---

## Decided
- A variant is written with a backtick before its shape, `` `red ``, `` `custom hue 0.5 ``; every unmarked call is a function, such as a named type's constructor or a Duration. The backtick is a symbol word, so `` `red `` is the call `` ` _ `` and the protocol grammar matches it as a rule; no consumer code reparses the tree.
- In a slot a variant needs parentheses, `` paint (`red) ``, and an `@` entry holding one is `` @(`by name) ``. After a keyword it stands alone: `` ERROR `missing ``, `` RETURNS `done "x" ``.
- Predicates (`WHERE [...]`, `REQUIRES [...]`) are decoded by a `term` rule in the grammar instead of from raw nodes. It needed the grammar language's `UNDERSCORE` pattern, for the `_` a predicate tests, since `_` isn't a word.
- The formatter hugs a symbol word to what follows it when it starts a run, or follows a keyword or a `#` line string, so it prints `` ERROR `missing ``, not `` ERROR ` missing ``.

## Tried and ruled out
- Committing the predicate grammar (#65) before the backtick (#64): its rules match the backtick, so the two went in one commit.

## Watch out
- A bare `true`, `null` or variant shape is now a function call: write `` `true ``. Errors suggest the marked variant ("`calm` is not a function; did you mean the variant `` `calm ``?").
- Messages quote code holding a backtick with double backticks; use `schema::code`, not a hand-written pair of backticks.

In the code: crates/anyx-protocol/protocol.grammar.anyx and request.grammar.anyx (`variant`, `term`, `term variant`), crates/anyx-protocol/src/refinement.rs, crates/anyx/src/format.rs (`prefix_hugs`), crates/anyx-protocol-generate/src/typescript_native.ts (`marked`), crates/anyx-shell/src/compile.rs.
