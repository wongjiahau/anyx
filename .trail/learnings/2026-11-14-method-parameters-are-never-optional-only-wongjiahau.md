---
title: Method parameters are never optional; only record fields are
author: Wongjiahau
date: 2026-11-14
branch: master
cost: { minutes: 30, tokens: 121102 }
id: l-33173917
scope: repo
status: active
src: agent
touches:
  - crates/anyx-protocol/PROTOCOL.md
  - crates/anyx-protocol-generate/README.md
  - crates/anyx-protocol-generate/src/lib.rs
  - crates/anyx-protocol-generate/src/rust.rs
  - crates/anyx-protocol-generate/src/typescript.rs
  - crates/anyx-protocol-generate/tests/fixtures/types.anyx
  - crates/anyx-protocol-generate/tests/generated.rs
  - crates/anyx-protocol-generate/tests/generated/types.rs
  - crates/anyx-protocol-generate/tests/generated/types.ts
  - crates/anyx-protocol-generate/tests/types_round_trip.rs
---

## Decided
- A method's parameters are always required. Only a record field can be `OPTIONAL` (and have a `DEFAULT`), because leaving out a slot's value breaks the method's sentence: `write "a" at revision content "…"` leaves `at revision` dangling. A record argument keeps the shape whole: `search "ode" with {}` and `search "ode" with { limit 10 }` both read as sentences.
- Something a caller may leave out is either a field of an options record, or a second method whose shape has the extra phrase: `list files` and `list files after (cursor: cursor)`. Pagination uses this pattern.

## Tried and ruled out
- Optional parameters (`list files (after: OPTIONAL: cursor)`), proposed for pagination: ruled out by the user because they break the natural reading of the shape.

## Watch out
- An optional *phrase* (words plus their slot, dropped together) does not break the reading; it is two shapes. It could come later as shorthand that expands into the two methods, but it isn't decided.

In the code: the check that rejects `DEFAULT`/`OPTIONAL` outside a record is in crates/anyx-protocol/src/check.rs; the rules are in crates/anyx-protocol/PROTOCOL.md.
