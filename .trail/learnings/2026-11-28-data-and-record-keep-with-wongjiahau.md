---
title: "DATA and RECORD keep WITH before their brackets"
author: Wongjiahau
date: 2026-11-28
id: l-3c9e5a41
scope: repo
status: active
src: agent
---

## Decided
- `DATA` and `RECORD` keep `WITH` before their square brackets, as in `DATA Shape RETURNS Type WITH [circle, square]`. They don't use `IS` and don't drop the word.

## Tried and ruled out
- Dropping `WITH`. The brackets would then come right after an expression, and a call may take a bare list (`find "x" in []`). So `DATA Shape RETURNS Type [circle, square]` also parses as the call `Type _` on a list. Only name resolution knows what is a type, so the parser can't tell the two apart. The brackets also clash with an optional part in a head, `[in (paths: …) DEFAULT in []]`.
- A different bracket for optional parts, `[* *]`. It fixes the optional-part clash but not the list clash, and needs a new bracket kind in the parser generator.
- `IS` instead of `WITH`. It parses, but `IS` means "equals" in `FUNCTION Key id RETURNS Type IS Text`, so `DATA Shape IS [circle, square]` reads as if `Shape` were that list. `WITH` reads as "a new type with these constructors", like `MATCH x WITH [cases]`.

## Watch out
- Making parentheses around every argument mandatory would remove the ambiguity, but it would force `find "x" in ([])`.

In the code: `core/core.grammar.anyx`, the `data` and `record` rules.
