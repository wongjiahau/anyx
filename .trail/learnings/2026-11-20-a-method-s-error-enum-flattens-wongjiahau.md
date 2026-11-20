---
title: "A method's error enum flattens the error groups it names"
author: Wongjiahau
date: 2026-11-20
branch: feat/choice-becomes-union
cost: { minutes: 1, tokens: 1811 }
id: l-41b8fe46
scope: repo
status: active
src: agent
touches:
  - crates/anyx-protocol-generate/src/rust.rs
---

## Decided
With `ERRORS [Export Errors, `too large]`, where `Export Errors` is `TYPE ALIAS Export Errors: UNION [`missing, `locked (path: Text)]`, the generated method enum is flat: `ExportError { Missing, Locked { path }, TooLarge }`. Generated code also has `From<ExportErrors> for ExportError`, so a shared helper that returns the group's enum still works with `?`. TypeScript gets a flat union discriminated by `kind`.

Why: the wire is flat (`missing` is written `` `missing ``), the checker already flattens nested unions, and matching stays one level deep. This is also how `ERRORS … + file errors` groups were generated before.

## Tried and ruled out
- Nesting, `ExportError { ExportErrors(ExportErrors), TooLarge }`: a second level to match through, and not how the value is written. Its one benefit, returning the group's enum from helpers, comes from the `From` impl instead.

In the code: crates/anyx-protocol-generate/src/rust.rs and typescript.rs, the per-method error enum (#59).
