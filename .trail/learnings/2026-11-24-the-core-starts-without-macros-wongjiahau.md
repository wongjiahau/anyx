---
title: "The dependently typed core starts without macros, implicit arguments or type classes"
author: Wongjiahau
date: 2026-11-24
id: l-c0e24b11
scope: repo
status: active
src: agent
---

## Decided
- The core is aimed at a real programming language, not just an IDL, but it starts as small as possible: `Type`, dependent function types written with `RETURNS`, holes as anonymous functions, calls, `FUNCTION`, `DATA` and `MATCH`. Macros (`MACRO`, `QUOTE`, `UNQUOTE`) come later.
- No `GIVEN` (implicit arguments), and no type classes or traits. Every argument is written out, and a "class" is a one-constructor `DATA` passed explicitly, as Haskell's dictionaries are under the hood.
- Why: implicit arguments, instance resolution and type inference exist for writability. Code is now mostly written by AI agents and read by people, so writing every argument out costs little and tells the reader everything. Dropping them loses no expressiveness: Lean's kernel has no implicit arguments either, since its elaborator fills them in first. This matches the protocol's "never infer types" rule.

## Watch out
- Without `GIVEN`, a type parameter written after the parameters that mention it, as in `(x: a) equals (y: a) in (a: Type)`, needs either parameters in any order without cycles, or a rule that the order follows dependencies. Not decided yet.
- Effects (monads as library types, or Koka-style effect rows) are deferred. Monads need no new core construct; effect rows would.

In the code: `core/core.grammar.anyx` on branch `feat/dependent-core-grammar` (PR #99) still has `MACRO`, `QUOTE`, `UNQUOTE`, the macro-use catch-all and `GIVEN`. Removing them is the first step.
