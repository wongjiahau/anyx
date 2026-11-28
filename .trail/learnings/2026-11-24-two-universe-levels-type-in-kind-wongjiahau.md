---
title: "The core has two universe levels, Type in Kind, and DATA states its level"
author: Wongjiahau
date: 2026-11-24
id: l-7b1d2e90
scope: repo
status: active
src: agent
---

## Decided
- Two universe levels, `Type: Kind`, and `Kind` has no type. It replaces `Type` in `Type`, which is inconsistent (Girard's paradox, Hurkens's short version): with it, any statement can be "proved", and type checking can loop.
- Predicative: a function type is in `Kind` when either side is, and a `DATA` in `Type` may only have constructor fields in `Type`, so a value in `Type` never contains a type. That is what blocks the paradox. This is the two-sort idea of Coquand's Calculus of Constructions, without Lean's infinite levels or level variables.
- `DATA` keeps `RETURNS`, and it says `Type` or `Kind`. The checker verifies the written level against the fields and never infers it: `RETURNS Kind` tells the reader the values hold types. Same rule as "never infer types", and every declaration still reads `name RETURNS type`.

## Tried and ruled out
- Dropping `RETURNS` from `DATA`: with `Type` in `Type` it was always `Type`, so redundant, but with two levels it carries the level, and inferring the level from the fields would hide it from the reader.
- Lean-style infinite levels with level variables (`Type u`): not needed yet, and level polymorphism adds noise to every generic signature.

## Watch out
- Cost: no `List of (Type)`, and polymorphic functions such as `(a: Type) RETURNS ((x: a) RETURNS a)` are in `Kind`, so they can't be stored or passed where a `Type`-level function is expected. Revisit only if real programs need it.
- Lean's impredicative `Prop` (for erasing proofs) is not included.

In the code: item 15 of `core/TODO.md`; the checker lives in `crates/anyx-core/src/kernel/`.
