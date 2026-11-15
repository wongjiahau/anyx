---
title: The prelude is checked by its own loader, not as a schema, and a type is written the way the formatter writes it
author: Wongjiahau
date: 2026-11-15
branch: work/prelude
id: l-3c5a8e21
scope: repo
status: active
src: agent
touches:
  - crates/anyx-protocol/src/prelude.rs
  - crates/anyx-protocol/src/schema.rs
  - crates/anyx-protocol/protocol.anyx
---

## Decided
- The prelude has no `PROTOCOL` header and only `INTRINSIC` declarations, so it is not a schema file: a small loader reads it with the schema grammar and checks it (each declaration implemented, documented, declared once, every built-in present). A broken prelude panics on first use, since it ships inside the crate and a test loads it.
- `INTRINSIC` is allowed on `RECORD`, `CHOICE`, `TYPE` and `ERRORS` in the grammar, then rejected by the checker outside the prelude. That way the message can say why, rather than being a parse error. It is not on `METHOD`, because a method already has 64 ways (`READ ONLY`, `IDEMPOTENT`, …) and an optional word would double them. `INTRINSIC METHOD` gets the same message from a hint in `decode_file`.
- A type's written name (`type_name_in`, used in typed method names and messages) follows the formatter: `LIST Text`, `MAP Integer`, but `LIST: LIST Text`. A `:` is kept only where the type after it starts with a keyword, since only then would the words join.
- Type parameters stay lowercase (`(item: TYPE)`), like other names. Generic types whose name starts with a slot, `(value) or (problem)` and `(item) dated`, keep lowercase words. Only a name that starts with words is written in Title Case: `Page of (item: TYPE)`.

## Tried and ruled out
- `bare: type name` as the second way of a named type's body: the generator reported an overlap with the typed way, and the decoder test fails on any warning. `ANYFIX CALL [name: WORDS [NO KEYWORD]]` doesn't overlap.

## Watch out
- Typed method names, which name an overload in JSON, now write `Float` where they wrote `FLOAT`, and `LIST Text` where they wrote `LIST: TEXT`. Values and generated identifiers are unchanged.
- The small inline schemas in `crates/anyx-protocol/tests/*.rs` keep lowercase type names; the fixtures, docs and generator tests use Title Case. A single-letter type name (`RECORD x`) can't be capitalized at all: `X` is a keyword.
