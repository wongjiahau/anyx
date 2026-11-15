---
title: "A call's optional words are groups decoded left to right by their first word, not one call per combination"
author: Wongjiahau
date: 2026-11-14
branch: work/optional-groups
id: l-88e24b0a
scope: repo
status: active
src: agent
touches:
  - crates/anyx-generator/README.md
  - crates/anyx-generator/grammar.anyx
  - crates/anyx-generator/src/analyze.rs
  - crates/anyx-generator/src/generate.rs
  - crates/anyx-generator/src/grammar.rs
  - crates/anyx-generator/src/lower.rs
  - crates/anyx-generator/src/model.rs
  - crates/anyx-generator/src/overlap.rs
  - crates/anyx-generator/src/support.rs
  - crates/anyx-generator/tests/check.rs
---

## Decided
- A call's parts may hold groups: `name: OPTIONAL [WORD "ERRORS", error list]`, `name: OPTIONAL: WORD "PUBLIC"`, and `name: ONE OF [value: returns, nothing: WORD "NOTHING"]` (a `ONE OF` with a `WORD` or `WORDS` choice, among call parts, is a choice between name words; at most one choice is a slot). `OPTIONAL: ONE OF [...]` may be none of them.
- The decoder gives one struct: `Option<T>` for a group with one unnamed value, `Option<Span>` for words alone, `Option<struct>` for named values, an enum for a choice. `OPTIONAL: WORD "PUBLIC"` is `Option<Span>`, not `bool`, so the word's place is kept for diagnostics and the LSP, like every other part.
- Decoding is greedy, LL(1) on the group's first word (`Sequence::choose`), inline in the call's decoder. So each group must start with a `WORD`, choices need distinct first words, and a group's first word must not be able to come right after it (checked in `lower.rs`, `check_groups`).
- The generator can list every way of writing the call (`model::expansions`, capped at 256). Expansion order: earlier groups vary slowest, present before absent, choices in written order, which keeps the protocol's old method form order. Errors, overlap analysis and `FORMAT` no longer go way by way: see "Two ways of a call are found competing by walking its graph twice" (l-4ffbc638).

## Tried and ruled out
- Backtracking over every combination (decoding each way as a separate call): simple, but exponential in groups and errors from failed ways compete.
- Choices of several parts each (`ONE OF [[WORD "A", x], ...]`): left out to keep each choice one part; a possible extension.
- `bool` for an optional word: loses the span.

## Watch out
- `WORDS` (any name words) can't be in or next to a group, since it would take the group's words.
- A group with no name is an error, and a group outside call parts is an error.
- Error lists used to grow with the number of ways (36 for the test fixture's method); they now name only the shortest ways.

In the code: crates/anyx-generator/src/model.rs (`Pattern::Group`, `Pattern::PartChoice`, `expansions`), src/lower.rs (`optional_group`, `part_choices`, `check_groups`, `way_errors`, `layout`), src/generate.rs (`Printer::group`, `Sequenced`, `Body::Group`), src/support.rs (`choose`, `call_of`, `span_since`), src/overlap.rs (`Overlap::expanded`), src/analyze.rs (`competing_ways`); tests in tests/fixtures/groups.anyx, tests/check.rs `groups_are_told_apart_by_their_first_word`.
