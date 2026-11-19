---
title: "A record field written as its variant alone is declared with a backtick slot, and @ goes away"
author: Wongjiahau
date: 2026-11-19
branch: feat/variant-backtick
cost: { minutes: 1, tokens: 9370 }
id: l-1c542fd0
scope: repo
status: active
src: agent
touches:
  - crates/anyx-protocol/protocol.grammar.anyx
  - crates/anyx-protocol/request.grammar.anyx
---

## Decided
- Drop `@`. With variants marked by a backtick, an entry that starts with one can't be a named field's shape, so it is the value of a field without a name: `{ `glob pattern, `any case, hidden }` instead of `{ @(`glob pattern), @(`any case), hidden }`.
- Such a field is declared the way it's written, a backtick before the slot: `` `(case: Case) DEFAULT `smart case ``. A bare `(case: Case)` already declares the named field `case`, and a keyword reads worse.
- It may only hold a type whose every value is a variant: a CHOICE, a union of variants, or one made nullable. No two such fields in a record may share a variant. JSON is unchanged, since fields keep their names there.

## Tried and ruled out
- Keeping `@` only in declarations: two spellings for one field. A keyword such as `VARIANT FIELD`: a new keyword where the backtick already says it.
- Text, numbers, lists and Durations in these fields: nothing real used them, and PROTOCOL.md already advised against `@"src"` because the reader can't tell what it means.
