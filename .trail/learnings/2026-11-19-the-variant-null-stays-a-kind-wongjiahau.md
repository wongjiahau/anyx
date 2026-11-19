---
title: "The variant null stays a kind object in JSON, with no special case"
author: Wongjiahau
date: 2026-11-19
branch: feat/named-type
cost: { minutes: 1, tokens: 2007 }
id: l-02d29645
scope: repo
status: active
src: agent
touches:
  - crates/anyx-protocol/src/json.rs
---

## Decided
- `null` is encoded in JSON as `{"kind": "null"}`, like every other variant. JSON gets no special treatment for any variant by its name.
- Readability decides: one rule (a variant is a `kind` object) is easier to understand than a rule with exceptions. In anyx text `null` reads the same either way.

## Tried and ruled out
- Plain JSON `null` for the variant `null`: one variant encoded unlike the others, while `true` and `false` stay objects.
- A way for a variant to choose its own JSON form: new syntax, and outside named types. If JSON primitives are ever wanted, `null`, `true` and `false` change together under a general rule.

In the code: crates/anyx-protocol/src/json.rs. Issues #51, #57.
