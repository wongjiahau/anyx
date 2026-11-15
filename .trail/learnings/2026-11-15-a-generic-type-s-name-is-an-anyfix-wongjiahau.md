---
title: A generic type's name is an anyfix shape whose type parameters can sit anywhere
author: Wongjiahau
date: 2026-11-15
branch: master
id: l-6e2b91f4
scope: repo
status: active
src: agent
touches:
  - crates/anyx-protocol/protocol.anyx
  - crates/anyx-protocol/PROTOCOL.md
  - crates/anyx-protocol/src/check.rs
  - crates/anyx-protocol/src/schema.rs
  - crates/anyx-protocol-generate/src/lib.rs
  - crates/anyx-protocol-generate/README.md
---

## Decided
- There is no `GENERIC` and no `WITH`. A generic RECORD or CHOICE names itself with a shape, like a method does, and each type parameter is a slot `(name: TYPE)`. The anyx spirit is that every call is anyfix, so **a slot can go anywhere in the shape: at the start, in the middle, at the end, or between words**. It is not only at the tail:
  ```anyx
  RECORD page of (item: TYPE) { items: LIST: item, next: OPTIONAL: cursor },        ## words, then a slot
  RECORD (item: TYPE) history { versions: LIST: item },                              ## a slot, then a word
  CHOICE (left: TYPE) or (right: TYPE) [ first: left, second: right ],               ## slot, word, slot
  RECORD table from (key: TYPE) to (value: TYPE) { rows: LIST: value },              ## two slots among words
  RECORD (value: TYPE) signed by (signer: TYPE) { value: value, signer: signer },    ## a slot first, then words, then a slot
  ```
- A use fills the slots in place, with type arguments in parentheses:
  ```anyx
  METHOD list files RETURNS (files: page of (file)),
  METHOD history of (id: document id) RETURNS (history: (document) history),
  METHOD parse (input: TEXT) RETURNS (parsed: (INTEGER) or (TEXT)),
  METHOD search (query: TEXT) RETURNS (results: page of ((document) signed by (person))),
  ```
  In a request, a value annotation will be written the same way: `find (x AS page of (person))` (issue #19, not implemented; the `reference` rule of the grammar is the type use it can reuse).
- Words such as `of`, `or` and `from` belong to the author's own type name, not to the protocol. That's why this is fine when a built-in `OF` keyword was rejected as domain-specific.
- Arguments are matched to parameters by their slot position. The shape's words say what each one is for, which is why named arguments are no longer needed.

## Tried and ruled out
- `GENERIC (item) RECORD page` used as `page WITH (item: T)`: it was implemented first (see the learning on GENERIC), but a prefix keyword plus named arguments goes against the anyfix spirit. The user rejected it.
- A built-in `OF` keyword, as in `page OF item`: domain-specific, and it only fits a slot at the tail.

## Watch out
- Type arguments need parentheses. `page of file` would read as one name, not `page of` plus a slot.
- `page` and `page of (…)` are different shapes, so both may exist.
- The checker rules from the GENERIC learning still apply: an unused parameter is an error, a parameter may not shadow a visible type, and self uses are limited to recursion through LIST or MAP.
- Generated names are the PascalCase of the shape's words, leaving out the slots, with type parameters in slot order: `PageOf<Item>`, `History<Item>`, `TableFromTo<Key, Value>`. A shape like `(left) or (right)` gives a poor name (`Or<Left, Right>`); whether authors need a way to name it is still open.
- Two shapes with the same words, such as `page` and `page (item: TYPE)`, or `page of (item: TYPE)` and `page (item: TYPE) of`, are fine to the checker but become one generated name, so the generator rejects them with its usual "which becomes the type `PageOf`, like …" error.
- `TYPE name: T` stays non-generic. With the grammar below, a slot there is a decode error ("expected the word `TEXT`, …"), not a checker error.
- `LIST page of (file)` works without the `:`, since `page of (file)` is a call with a slot, not keywords side by side.
- A scoped use `common/page of (file)` parses as the call `_ / _` of `common` and `page of _`, so the checker takes a slashed path apart: every segment but the last is a plain word, and the last one carries the type arguments.

In the code:
- protocol.anyx: `record body`/`choice body` are `ANYFIX CALL [NAME [NO KEYWORD], ARGUMENTS: ZERO OR MORE: record part]`, a part being the braces or a `type parameter` (`name: WORDS`, `marker: WORD "TYPE"`). A type use is `reference: ANYFIX CALL [NAME [NO KEYWORD], ARGUMENTS: ZERO OR MORE: type]`. The braces may sit in any slot to the grammar; check.rs (`type_shape`, `declare_shaped_type`) takes the last one as the body and reports braces anywhere else, and a missing body.
- `TypeDefinition.name` is the shape text with `_` per slot (`page of _`, `_ or _`) and `parameters` are in slot order. `schema::fill_slots` and `TypeDefinition::shape()` show it as `page of (item)`; messages use those.
- check.rs: `type_use` resolves a use via `path` (scope and shape) and `resolve_type`. When no type has that shape, `resolve_type` lists the types with the same words ("found `page (TEXT)`, with a type in 1 slot, but … has no type of that shape; the types with its words are …"), else it adds the types that start with the same first word, and notes when the first word is a type parameter.
- Regenerating grammar.rs in release is ~30s: `cargo run -q --release -p anyx-generator --bin anyx-generate -- crates/anyx-protocol/protocol.anyx`. The test that checks it is up to date takes ~10 minutes in debug.
