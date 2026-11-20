---
title: "A method always writes ERRORS, and ERRORS [] when it has none"
author: Wongjiahau
date: 2026-11-20
branch: feat/choice-becomes-union
cost: { minutes: 1, tokens: 1365 }
id: l-0407b67c
scope: repo
status: active
src: agent
touches:
  - crates/anyx-protocol/protocol.grammar.anyx
---

## Decided
Every METHOD writes `ERRORS [ … ]`; a method that can't fail writes `ERRORS []`, the empty union. Leaving `ERRORS` out is an error, and there is no `NO ERRORS` keyword.

Why: a reader, usually an agent, sees on every method that errors are declared up front, and can tell "can't fail" apart from "the author forgot its errors". It matches `RETURNS NOTHING`, which already makes "returns nothing" explicit. Writing `ERRORS []` costs only writability.

## Tried and ruled out
- `ERRORS` optional, with omission meaning `[]` (#59's first text): hides whether errors were considered.
- A `NO ERRORS` keyword: `[]` already says it, as `UNION []` is the union with no values.

In the code: issue #59, the `method` rule in crates/anyx-protocol/protocol.grammar.anyx (`errors: OPTIONAL [WORD "ERRORS", error list]` becomes required).
