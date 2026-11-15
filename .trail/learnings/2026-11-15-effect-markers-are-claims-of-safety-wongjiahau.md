---
title: "Effect markers are claims of safety: READ ONLY, IDEMPOTENT before METHOD, UNDONE BY a typed call after ERRORS"
author: Wongjiahau
date: 2026-11-15
branch: work/effects
id: l-5e1f0c7b
scope: repo
status: active
src: agent
touches:
  - crates/anyx-protocol/protocol.anyx
  - crates/anyx-protocol/src/check.rs
  - crates/anyx-protocol/src/schema.rs
  - crates/anyx-protocol/src/client.rs
  - crates/anyx-protocol/src/guide.rs
  - crates/anyx-protocol-generate/src/lib.rs
  - crates/anyx-protocol-generate/src/rust.rs
  - crates/anyx-protocol-generate/src/typescript.rs
---

## Decided
- `READ ONLY METHOD …` and `IDEMPOTENT METHOD …` (issue #16). No marker is the worst case (may change anything, not safe to retry, can't be undone). `READ ONLY` implies idempotent, so `Effect` is one enum (`Unmarked`, `Idempotent`, `ReadOnly`) with `idempotent()` true for both markers. IDEMPOTENT is about the state, not the response (RFC 9110).
- The grammar reads `READ ONLY` and `IDEMPOTENT` as two separate optional words, not one `ONE OF`, so that `READ ONLY IDEMPOTENT METHOD` parses and the checker can say why it's wrong instead of a generic decode error.
- `UNDONE BY` comes after `ERRORS` and before `EXAMPLES`: RETURNS and ERRORS (the outcomes) stay together and EXAMPLES stays last. The undo is the `shape` rule itself (`undo: shape`): `UNDONE BY move (new path: TEXT) to (path: TEXT)`. Each slot names a parameter of the method being declared and repeats its type; the types pick the method among those with the same words, so the lookup stays right once overloading lands. Only parameters, no returned values or literals, for now.
- An undo is resolved **in the method's own scope** only: a template is a shape, which can't hold a `/` path. A method of another scope can't be named yet; the "no method" error says which scope was searched.
- `UNDONE BY` also claims the call disclosed nothing outside the account that undoing wouldn't take back (no separate OUTWARD marker). Written in PROTOCOL.md and the guide.
- Generated code exposes markers as data: Rust `pub const READ_EFFECTS: anyx_protocol::client::Effects` per method in its scope's module; TypeScript `export const EFFECTS` keyed by the method's full name (what `call` sends), plus `SCHEMA`. `Effects` is now a reserved type name. There's no retry helper in either client, so none was added.

## Tried and ruled out
- `effect: OPTIONAL: ONE OF [read only: WORDS "READ ONLY", idempotent: WORD "IDEMPOTENT"]`: shorter, but makes the double marker a parse error with no explanation.
- `OPTIONAL [WORDS "UNDONE BY", undo]`: `FORMAT` can only `BREAK BEFORE WORD`, so it is `WORD "UNDONE", WORD "BY"`.
- Generated tests that check claims by running EXAMPLES twice: no generated test harness or server state snapshot exists to compare against, so it was skipped.

## Watch out
- The method rule now has 64 ways of writing it (128 with the doc), so the decoder's "expected the call …" error lists are long, and regenerating `src/grammar.rs` is slow.
- Undos are checked after every method is read and before `check_recursion`, so a slot's unknown type is reported there, after other type errors.

In the code: `method` and `undo` in crates/anyx-protocol/protocol.anyx; `Checker::method` (effect) and `Checker::undo` in src/check.rs; `Effect`, `Undo`, `Schema::typed_shape` in src/schema.rs; `client::Effects`; the Effects section in src/guide.rs and PROTOCOL.md; `effects_doc` in crates/anyx-protocol-generate/src/lib.rs, `effects` in rust.rs and typescript.rs; tests in crates/anyx-protocol/tests/effects.rs and tests/snapshots/effect_diagnostics.txt.
