---
title: "A base type doesn't fit its refinement"
author: Wongjiahau
date: 2026-11-20
branch: feat/subtyping
cost: { minutes: 1, tokens: 1460 }
id: l-ba3a724a
scope: repo
status: active
src: agent
touches:
  - crates/anyx-protocol/src/schema.rs
  - crates/anyx-shell/src/compile.rs
  - crates/anyx-protocol/src/codec.rs
---

## Decided
A refinement fits its base, but the base doesn't fit the refinement. With `TYPE ALIAS Depth: Integer WHERE [0 <= _]` and `METHOD count RETURNS (count: Integer)`, the shell rejects `dig (count)` before running it, rather than running the predicate when the value arrives. A literal such as `dig 3` still works, since its value is known and checked against the predicate up front: that checks a value, not a type fitting a type.

Why: a type should promise something a reader can rely on (readability over writability), and it pushes a protocol to declare precise return types, as `ERRORS` must be declared up front. The user chose this over letting a computed base value through to a run-time predicate check (#60, stage 6 of #51).

## Watch out
Letting a literal's own type pick an overload and then checking its value stays lenient. Only a computed value's static type fitting a parameter is strict. Keep the two jobs apart.

In the code: `Schema::fits` (strict) and `Schema::fits_literal` (a base fits its refinement) in crates/anyx-protocol/src/schema.rs. codec.rs uses `fits_literal`. crates/anyx-shell/src/compile.rs picks between them by whether the argument is a literal. The rules are in PROTOCOL.md under Subtyping.
