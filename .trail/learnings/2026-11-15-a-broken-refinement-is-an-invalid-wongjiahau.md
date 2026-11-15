---
title: An argument that breaks a refinement or precondition makes the whole request invalid, with a specific message
author: Wongjiahau
date: 2026-11-15
branch: master
id: l-4c81e07a
scope: repo
status: active
src: agent
touches:
  - crates/anyx-protocol/PROTOCOL.md
---

## Decided
- A request whose argument breaks a refinement type's predicate (#11) or a method's precondition (#12) is `INVALID REQUEST` up front, so none of its calls run. A predicate is part of the parameter's type, so breaking it is treated like a type error or an ambiguous call (#19).
- The message must be specific, never generic. It names:
  - the call and argument, with their line;
  - the value sent;
  - the type and predicate, as written and in words;
  - for a chain like `0 < limit <= 100`, which bound failed.
  The aim is that an agent can fix the request in one try.
- Enforcement lives in the codec's decoder, so every generated server enforces it whatever its target language. Generated clients may check the same predicate before sending.
- A returned value that breaks a refinement or postcondition is the server's bug: `INTERNAL ERROR`.
- Even if generated code didn't enforce them, refinements and conditions are worth having as documentation, because they tell callers which values are right.

## Tried and ruled out
- An `ERROR` for that call alone, with the rest of the request running: a broken predicate is a malformed request, not a domain failure.

In the code: not implemented yet; see issues #11 and #12.
