---
title: A value with no type of its own is always annotated with AS:, even without overloading
author: Wongjiahau
date: 2026-11-15
branch: master
id: l-19a5c0de
scope: repo
status: active
src: agent
touches:
  - crates/anyx-protocol/PROTOCOL.md
---

## Decided
- Methods may be overloaded: the same shape with different parameter types, resolved per call by the arguments' types (issue #19).
- In a request, a value whose type can't be read from the value alone must be annotated in the argument position. This covers `[]`, `{}`, and any value passed as a named type. The annotation is required even when the method has no overloads, so adding an overload later never breaks an existing call. Examples: `tag ([] AS: LIST: TEXT)`, `open ("abc" AS: file id)`. Without the annotation, the whole request is `INVALID REQUEST` up front.
- Overloads that no annotation could tell apart (a generic one versus a concrete one, or identical parameter types) are a schema error, like Rust's coherence rule.
- The annotation is the call `_ AS _`. `AS: x` is only `:` sugar for `AS (x)`; the colon matters only before an all-caps type, where `AS LIST` would otherwise join into one keyword name. The annotation is a checked claim, not a conversion.

## Tried and ruled out
- `("abc": file id)` and `(file id: "abc")`: in a value position, `a: b` already means `a (b)`, and a value may be any anyx expression. `:` is a name annotation only in a parameter, where the part before it is always a name.
- Accepting `open "abc"` for a named type, and rejecting only calls that several overloads fit: adding an overload that takes the underlying type would break those calls later.

In the code: not implemented yet; see issue #19.
