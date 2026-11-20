---
title: "A method's errors are variants, alone or through aliases of unions of variants"
author: Wongjiahau
date: 2026-11-20
branch: feat/choice-becomes-union
cost: { minutes: 1, tokens: 1750 }
id: l-73feb500
scope: repo
status: active
src: agent
touches:
  - crates/anyx-protocol/src/check.rs
---

## Decided
`ERRORS [ … ]` holds variants, and aliases of unions of variants (such as `File Errors`). Such an alias may itself include other such aliases, as `` TYPE ALIAS File Errors: UNION [`trashed, Common Errors] ``, which is what `ERRORS file errors [`trashed] + common errors` was: a union of unions of variants is still a union of variants, flattened, and keeps its name. This follows from flattening and from #51, which counts another union through an alias as a union member. A named type, an intrinsic such as `Text`, a refinement or any other type there is an error, whose message says to write a variant with a slot instead, as in `` `conflict at (revision: Integer) ``.

Why: an error should say what went wrong, and a variant's words do; a bare `Text` or `Integer` error tells a reader, often an agent, nothing. It keeps the flat per-method enum and the JSON `kind` form as they are. Widening later to any union member is not breaking; narrowing back would be.

## Tried and ruled out
- Any union member, with a wrapping enum variant per named type or intrinsic (`ConflictAt(ConflictAt)`, `Text(String)`) and responses like `ERROR "oops"`: errors that don't say what failed.

In the code: the method's errors check in crates/anyx-protocol/src/check.rs (#59).
