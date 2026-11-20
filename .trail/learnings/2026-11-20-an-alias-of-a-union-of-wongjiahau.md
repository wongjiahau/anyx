---
title: "An alias of a union of variants keeps its name in the checked schema; other aliases don't"
author: Wongjiahau
date: 2026-11-20
branch: feat/choice-becomes-union
cost: { minutes: 10, tokens: 31575 }
id: l-1284eac1
scope: repo
status: active
src: agent
---

## Decided
- When `CHOICE` becomes a union of variants (#58), `TYPE ALIAS Case: UNION [`smart case, `any case]` stays in the checked schema as a named definition, as a `CHOICE` did. Generated code needs the name for its enum (`pub enum Case`), and the guide and error messages say `Case` rather than the whole union.
- The checker still reads through it to its variants, so it behaves as an alias: its values are its variants, written the same.
- Any other alias (`TYPE ALIAS Paths: LIST Text`, `UNION [Integer, `none]`) leaves no trace in the checked schema; nothing needs its name.

## Tried and ruled out
- Keeping every alias in the schema as a reference type: uniform, but every place that inspects a type (checker, decoders, JSON, shell, both generators) would have to resolve it first, and a missed one is a quiet bug.
