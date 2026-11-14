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
  In a request, a value annotation is written the same way: `find (x AS page of (person))`.
- Words such as `of`, `or` and `from` belong to the author's own type name, not to the protocol. That's why this is fine when a built-in `OF` keyword was rejected as domain-specific.
- Arguments are matched to parameters by their slot position. The shape's words say what each one is for, which is why named arguments are no longer needed.

## Tried and ruled out
- `GENERIC (item) RECORD page` used as `page WITH (item: T)`: it was implemented first (see the learning on GENERIC), but a prefix keyword plus named arguments goes against the anyfix spirit. The user rejected it.
- A built-in `OF` keyword, as in `page OF item`: domain-specific, and it only fits a slot at the tail.

## Watch out
- Type arguments need parentheses. `page of file` would read as one name, not `page of` plus a slot.
- `page` and `page of (…)` are different shapes, so both may exist.
- The checker rules from the GENERIC learning still apply: an unused parameter is an error, a parameter may not shadow a visible type, and self uses are limited to recursion through LIST or MAP.
- Generated names come from the shape's words, such as `PageOf<Item>` and `History<Item>`. A shape like `(left) or (right)` gives a poor name (`Or<Left, Right>`), so this needs a rule.

In the code: not implemented yet; it replaces the GENERIC grammar forms and `generic`/`generic_use` in check.rs.
