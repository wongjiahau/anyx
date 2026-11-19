---
title: "The protocol language never infers types: every value is checked against a type the schema states"
author: Wongjiahau
date: 2026-11-19
branch: feat/union
cost: { minutes: 1, tokens: 827 }
id: l-bd2d312b
scope: repo
status: active
src: agent
touches:
  - crates/anyx-protocol/src/schema.rs
  - crates/anyx-protocol/src/check.rs
---

## Decided
- The anyx protocol language has no type inference beyond checking a value against an expected type the schema already states. Inference mostly serves writability; types in a schema are documentation, and readability wins (see AGENTS.md design priorities).
- Choosing a union member, an `@` field or an overload must be one step from the value's outermost form, never a search, unification or backtracking.
- Where a value is ambiguous, the writer annotates it (`(true AS Boolean)`) rather than the checker working it out from context.

## Tried and ruled out
- OCaml-style open polymorphic variants with row variables: unions are always closed and written out in full.
- Finding a member or field by trying each candidate type: it makes decoding a search.

## Watch out
- Overload resolution by argument type is where inference would creep in; anything needing unification gets an annotation instead.

In the code: crates/anyx-protocol/src/schema.rs (`member_written_as`, `member_holding`), crates/anyx-protocol/src/check.rs (`check_union`).
