---
title: Never parse on top of the tree the generated parser gives; read its structure or extend the grammar
author: Wongjiahau
date: 2026-11-19
branch: feat/fd-protocol-tool
cost: { minutes: 4, tokens: 19817 }
standing: true
id: l-c050201f
kind: guardrail
scope: repo
status: active
src: agent
touches:
  - crates/anyx-protocol/src/time.rs
  - TODO.md
  - crates/anyx-fd/README.md
  - crates/anyx-fd/fd.protocol.anyx
  - crates/anyx-fd/src/lib.rs
  - crates/anyx-fd/tests/fd.rs
  - crates/anyx-protocol/PROTOCOL.md
  - crates/anyx/src/node.rs
  - crates/anyx-protocol/src/schema.rs
---

## Decided
- Consumers of anyx never add parsing of their own on top of a tree from a parser the Anyx parser generator made. That covers re-reading source text, and also turning part of the tree into a string and splitting the string back apart.
- What a consumer needs from syntax goes in the Anyx grammar, so the generated parser produces it. What the grammar can't express is read from the tree's structure as it is: a call's `name: Vec<NamePart>` of words and slots, its `arguments`, its node kinds.
- The user asked for this to hold always: fully use the parser generator.

## Tried and ruled out
- `time::is_duration_shape` took `Call::name_text()` (`"_ hours _ minutes"`), split it on spaces, and checked slots and units alternate, to see whether an `@` entry may be a `Duration`. It worked only because `_` is never a word. The call's `name` parts already said the same thing.

## Watch out
- `name_text()` is a key for lookups and messages, such as a variant's shape. It isn't a format to parse back.
- The splits in `anyx-generator` and the generated `grammar.rs` are the generator's own work, which is where parsing belongs.

In the code: crates/anyx/src/node.rs (`Call`, `NamePart`, `name_text`), crates/anyx-protocol/src/schema.rs (`Written::of`, `Written::admits`), crates/anyx-protocol/src/time.rs (`is_duration_shape`). AGENTS.md says the same under "Anyx syntax and parsing".
