---
title: "Two ways of a call are found competing by walking its graph twice, not by comparing every pair of ways"
author: Wongjiahau
date: 2026-11-15
branch: work/overlap
id: l-4ffbc638
scope: repo
status: active
src: agent
touches:
  - crates/anyx-generator/src/analyze.rs
  - crates/anyx-generator/src/overlap.rs
  - crates/anyx-generator/src/model.rs
  - crates/anyx-generator/src/lower.rs
  - crates/anyx-generator/tests/check.rs
  - crates/anyx-generator/tests/generated.rs
  - Cargo.toml
---

## Decided
- `competing_ways` stays, but walks the call's graph (`overlap::call_graph`: steps in a row, each group a branch point) twice side by side, once down each of two branches of one branch point, and asks whether both walks take the same items (`Overlap::fork`). Node patterns are compared once per pair (`Known`). Overlap between two calls walks their graphs the same way, groups and all. Generating the protocol's decoder went from about 546s to about 1.5s (issue #23).
- Errors name a call with groups by its shortest ways, every optional group left out (`model::shortest_ways`): `expected the call `_? METHOD _ RETURNS _`, `_? METHOD _ RETURNS NOTHING` …` instead of all 64 ways. The decoder already says what it expected where a call goes wrong further in. LSP completions offer those shortest ways too, so no longer `PUBLIC RECORD _` as a whole.
- A call's `FORMAT` is one layout rule naming the words every way has, since breaking before a word that is not in the call changes nothing; it falls back to a rule per way only when a word to break before is first in some way.
- `opt-level = 3` for the generator and parser in dev builds was reverted: it did not help, and nearly doubled building them.

## Why the checks on groups don't make `competing_ways` redundant
`check_groups` makes a word decide each group: where one way has a group and another doesn't, the item after the same parts is the group's word in one and something else in the other. That holds only while a word decides and the parts before take the same items in both. Two cases are left, both in tests/snapshots/check_groups.txt:
- a slot choice of a `ONE OF` between groups, decided by a node, followed by parts that may start with a node: `F _ _?` and `F _?` both fit `F "s"`;
- parts before a group that take more or fewer nodes, which moves the group's word to where a later word stands: `_* X _ _*` and `_* _ X _*` both fit `a X b`.

## Watch out
- `Known` is only sound while nothing else is being compared (the `active` stack is empty), since a comparison cut short at a recursive rule is not final. `walk` makes a fresh one each time; only `competing_ways` shares one across its walks.
- Errors from `way_errors`, layout and `Overlap::expanded` (a call with groups against a word or `ANYFIX CALL [NAME ...]`, and `covers`) still go through `expansions`, once each, which is linear in the ways and capped at 256.
