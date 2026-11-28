---
title: "A record field written as its value alone is marked with a dot"
author: Wongjiahau
date: 2026-11-28
id: l-5a7c1e39
scope: repo
status: active
src: agent
---

## Decided
- A record field written as its value alone carries a `.` mark, both where it's declared and in each value: `RECORD Search WITH [.(case: Case), max depth (n: Natural)]` and `{ .any case, max depth 3 }`.
- Why: without the mark, `{ 2 hours, max depth 3 }` reads as two fields, `_ hours` and `max depth _`. In fact `2 hours` is a value of the unnamed `Age` field. A reader would need the record's declaration open to tell. The mark costs one character, and readability comes before writability.
- Records stay nominal. The value's record is still found from its entries.

## Tried and ruled out
- **Backtick (`` `any case ``):** the protocol used it for unnamed fields. It clashes with code spans in doc comments.
- **`'`, `*`, `=`, `•`:** `'` and `•` were close. `.` reads like a list bullet, is easy to type, and is free now that grouping uses `⁽` and `₎`.
- **Parentheses around unnamed entries:** it's unclear whether the tree keeps them distinct from plain grouping.
- **Structural records:** these would force the mark anyway. They were dropped because two records with the same fields would become the same type, losing the safety names give.

## Watch out
- **Matching cost:** finding a value's record scans every record in scope, `O(R · e · f)`. That's fine at today's sizes. If one scope ever holds thousands of records, index the records by field words and by `.` type rather than changing the syntax.

In the code:
- `core/core.grammar.anyx`, the `field shape` and `entry` rules.
- `crates/anyx-core/src/lowering.rs`, `fitting_record` and `fill`.
- `crates/anyx-core-generate/src/client.rs`, which writes `.` before such an entry.
