---
title: "A record's fields are shapes, and @ marks a field written as its value alone"
author: Wongjiahau
date: 2026-11-19
branch: feat/fd-protocol-tool
cost: { minutes: 1, tokens: 1260 }
id: l-d0a6cdee
scope: repo
status: active
src: agent
touches:
  - crates/anyx-protocol/protocol.grammar.anyx
  - crates/anyx/src/format.rs
  - crates/anyx-fd/fd.protocol.anyx
---

## Decided
- Every record field is a shape whose slots hold types, written in a value as declared: `path "a"`, `(any) case`, `between 1 and 3`. `x: T` is `x (T)`, so there is no separate "named" kind. Words alone are a checkbox (Boolean, true when written, no DEFAULT). Several slots hold a tuple, one value each (JSON array, Rust tuple, TypeScript tuple); a tuple has no syntax of its own.
- `DEFAULT` takes the whole field, `_ DEFAULT _` at the root of the tree, followed by the entry the field holds when left out, as a value writes it: `age (Integer) DEFAULT age 0`, `@(order: Order) DEFAULT @newest first`. With a colon the field goes in parentheses, `(age: Integer) DEFAULT age 0`. The default is decoded as an entry of its field, by the request decoder.
- `@(name: T)` is a field written as `@` and its value, `@by name`. A symbol prefix, so it's marked at the call site and costs one character; the declaration and the call site share the syntax.
- An `@` entry is found by its value's outermost form alone (variant shape, string, number, list, braces, duration). The checker rejects two `@` fields sharing any form, even one variant shape; Integer/Float clash, Text/Instant clash. This keeps decoding a lookup, never a search. The user asked for this strictness explicitly.
- Guideline (PROTOCOL.md, Records): checkbox for on/off; separate fields for options that combine; a shape by default (always for text, numbers, lists); the shared word in the shape (`(any) case`); `@` only for self-describing phrases with no shared word, where a word would stutter. When in doubt, a shape. The user: "sometimes having the key is clearer".
- `(any) case` keeps its parentheses: readability over writability. The decoder hints at it when `any case` is written.

## Tried and ruled out
- `UNNAMED` keyword: too heavy, and nothing marked the value at the call site.
- A parameter in a field's slot (`max depth (depth: Depth?)`): a lowercase name is ambiguous with a generic's type parameter in the grammar.
- A bare value after the shape (`(Case) case DEFAULT smart`), then `DEFAULT` inside the slot's type (`(Case DEFAULT smart) case`): with several slots, a slot's default needed an "every slot or none" rule. The user put `_ DEFAULT _` at the root instead.
- One slot per field, with a record or choice for a pair: the user wanted several values per field, a tuple in generated code.
- A nullable field written as its item (`max depth 3` for `Depth?`): an implicit coercion; see the learning on no syntax sugar.

## Watch out
- Adding a variant to a choice used by an `@` field can make the record clash; the checker names both fields.

In the code: crates/anyx-protocol/protocol.grammar.anyx (`field shape`), src/check.rs (`check_forms`, `field`, `read_defaults`), src/schema.rs (`Written::admits`, `shaped_field`, `unnamed_field`), src/request.rs (`entry_field`, `decode_field_entry`), src/codec.rs (`write_fields`), crates/anyx/src/format.rs (`prefix_hugs`), crates/anyx-fd/fd.protocol.anyx (`RECORD Search`).
