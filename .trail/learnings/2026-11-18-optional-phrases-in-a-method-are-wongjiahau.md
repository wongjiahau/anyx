---
title: "Optional phrases in a method are left out: free modifiers go in a list of settings, and a clause that changes the contract is its own method"
author: Wongjiahau
date: 2026-11-18
cost: { minutes: 1, tokens: 2695 }
id: l-54530c3f
scope: repo
status: active
src: agent
---

## Decided
The protocol does not get optional phrases (`METHOD search (regex: Text) [in (paths: LIST Text)] [matching (case: Case)]`). They are only a shortcut for writing out one method per combination, and the wire format is the same either way, so adding them later would break nothing.

Where an optional part goes instead:
- Modifiers that combine freely (ripgrep's `-w -x -v -F …`) are a list of settings: a CHOICE whose variants are anyfix shapes, `with [any case, whole words, "latin1" encoding]`. It is a value, so it can be shared, kept and computed, and it comes in any order. Conflicts and repeats are ruled out with `REQUIRES`.
- One or two optional sentence parts (`in (paths)`) are methods written out: `search _` and `search _ in _`.
- A clause that changes the result or the errors (`list files with sizes`, `write … unless changed since revision …`) is a different contract, so it is a separate method.

## Tried and ruled out
- Options records with DEFAULT fields for the modifiers (`with { case: any case, whole: word }`): they read as data, not a sentence.
- Optional phrases that change the return type or errors: each combination is then a different contract, and generated code would need a function per combination anyway.

## Watch out
- Should optional phrases ever be added, every combination must share one result and one set of errors. What would justify them is three or more optional sentence-level clauses that need their own names (in conditions) and share one contract.
- A list of settings relies on prelude functions over lists, such as `(items) has no repeats` and `(items) has at most one of (choices)`, and on variants written as constants in conditions.
