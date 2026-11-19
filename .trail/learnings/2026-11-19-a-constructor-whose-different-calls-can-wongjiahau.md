---
title: "A constructor whose different calls can mean one value is a function, not a variant"
author: Wongjiahau
date: 2026-11-19
branch: feat/named-type
cost: { minutes: 10, tokens: 139950 }
id: l-4eba9e45
scope: repo
status: active
src: agent
touches:
  - crates/anyx-protocol/prelude.protocol.anyx
  - crates/anyx-protocol/src/time.rs
---

## Decided
- Duration's constructor is an intrinsic prelude FUNCTION returning `Duration`, an `INTRINSIC TYPE`, not a variant. A variant is a free constructor, so `1 hours 30 minutes` and `90 minutes` would be different values and need a normalization rule in the checker and interpreter. A function computes a value, so equality is on the result, and JSON carries the value (milliseconds), never the call.
- The rule in general: a variant, or a named type's constructor (`File Id "a"`), is free, one call one value, and JSON keeps it. A constructor with equations between calls is a function.
- Only intrinsic prelude functions may appear in a value; a user FUNCTION never reaches the wire. Decoding stays a cheap evaluation with no inference.
- A type's forms for telling union members apart are its constructor functions' shapes, so the special duration form goes.

## Tried and ruled out
- Duration as `UNION [one variant with optional parts]`, as #54 first had it.

## Watch out
- The encoder needs the inverse (one spelling, largest unit first); it stays intrinsic but is declared in the prelude.

In the code: crates/anyx-protocol/prelude.protocol.anyx (`INTRINSIC CHOICE Duration`), crates/anyx-protocol/src/time.rs. Issues #54, #51.
