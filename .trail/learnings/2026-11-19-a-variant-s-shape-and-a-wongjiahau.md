---
title: "A variant's shape and a method's shape are the same syntax, and every upgrade to one applies to both"
author: Wongjiahau
date: 2026-11-19
branch: feat/fd-protocol-tool
cost: { minutes: 3, tokens: 56038 }
standing: true
id: l-78a7ae1c
kind: guardrail
scope: repo
status: active
src: agent
touches:
  - crates/anyx-protocol/src/check.rs
  - crates/anyx-protocol/src/schema.rs
  - crates/anyx-protocol/protocol.grammar.anyx
  - crates/anyx-fd/fd.protocol.anyx
---

## Decided
- A variant (`(path: Text) does not exist`) and a method (`find (pattern: Text) in (paths: LIST Text)`) are both calls, and their shapes are one syntax. Every upgrade applies to both, in the grammar, the checker, the decoder and the generated code. The user asked for this to always hold.
- The first upgrade under this rule is optional parts, written in square brackets inside a shape: `[(days: Integer) days] [(hours: Integer) hours]` for `Duration`, or `find (pattern: Text) [in (paths: LIST Text)] [with (search: Search)]`. Order stays fixed.
- Each optional part has a `DEFAULT`, as a record field does, inside its brackets and followed by the part as a call writes it: `[in (paths: LIST Text) DEFAULT in ["."]]`. The handler always gets a value, and the generated parameter is `T`, not `T?`. The user chose this over an omitted part holding `null`.
- An entry with every part left out needs no rule, since it can't be written. `changed within` is another shape, and `changed within ()` is an empty parentheses node, not a call.

## Tried and ruled out
- Adding a feature to variants alone, or to methods alone. Two shape syntaxes would drift apart.

## Watch out
- Open: `1 hour` versus `1 hours`. The singular would be sugar, which needs the owner's approval.

In the code: crates/anyx-protocol/src/check.rs (`shape`, which methods and variants already share), crates/anyx-protocol/src/schema.rs (`Shape`), crates/anyx-protocol/protocol.grammar.anyx, crates/anyx-fd/fd.protocol.anyx (twelve methods that optional parts would make three).
