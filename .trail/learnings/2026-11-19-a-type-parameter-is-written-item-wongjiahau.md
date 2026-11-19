---
title: "A type parameter is written (Item: TYPE): the marker is a keyword, not a type"
author: Wongjiahau
date: 2026-11-19
branch: feat/type-alias
cost: { minutes: 35, tokens: 128282 }
id: l-3e04df32
scope: repo
status: active
src: agent
touches:
  - crates/anyx-protocol/protocol.grammar.anyx
  - crates/anyx-protocol/src/check.rs
  - crates/anyx-protocol/prelude.protocol.anyx
---

## Decided
- A type parameter is written `(Item: TYPE)`, for `RECORD`, `CHOICE` and `TYPE ALIAS` alike. Its name is Title Case, and its marker is the keyword `TYPE`.
- The user chose the keyword so that `Type` is never a type. If it were, it would need a type of its own (`Type : Type`), and that leads to type universes.

## Tried and ruled out
- `(Item: Type)`, as #51 first had it. In a `TYPE ALIAS` name, `(Item: Type)` is the same anyx tree as a use of a generic type `Item _` applied to a type named `Type`. The generated parser can't tell the type parameter from the aliased type, and the generator warns about the overlap. Telling them apart in the checker would mean parsing on top of the tree.
- Teaching the grammar language to exclude a word, `NAME [NOT "Type"]`, so that `Type` is never a reference. The keyword is simpler, and needs no change to the generator.

## Watch out
- With Title Case type names, the bare variant slot `value (item)`, which took its parameter name from its type, could no longer name a parameter. It was removed from the grammar rather than lowercasing the type's name in the checker, so the prelude writes `value (item: Item)`.

In the code: crates/anyx-protocol/protocol.grammar.anyx (`type marker`, `alias part`), crates/anyx-protocol/src/check.rs (`type_parameter_names`, `check_type_parameter_words`).
