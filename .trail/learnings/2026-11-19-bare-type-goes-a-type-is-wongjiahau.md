---
title: "Bare TYPE goes: a type is either a TYPE ALIAS or a NAMED TYPE with a constructor"
author: Wongjiahau
date: 2026-11-19
branch: feat/named-type
cost: { minutes: 1, tokens: 1520 }
id: l-5458fb95
scope: repo
status: active
src: agent
touches:
  - crates/anyx-protocol/protocol.grammar.anyx
---

## Decided
- `TYPE Name: T` is removed. Each one becomes a `TYPE ALIAS` (transparent, written as `T`) or a `NAMED TYPE` (nominal, written with its constructor, `File Id "abc"`).
- Readability is the protocol's primary goal. Languages disagree on what a bare `type` means (an alias in Haskell, Rust, OCaml, TypeScript; nominal in Go and Elm), so two explicit forms spare the reader a guess.
- Most languages give a nominal type a constructor (Haskell `newtype`, Rust `struct`, Elm), so a value shows its type at the call site. A nominal type written untagged is Go's model, and it is what made the shell need `AS:`.
- Rule of thumb: `NAMED TYPE` when mixing two values up would be a bug (ids such as `File Id`); `TYPE ALIAS` when it is only a constrained value (fd's `Depth`, `Size`, `Count`).

## Tried and ruled out
- Keeping bare `TYPE` as a third form, nominal but untagged: a value still wouldn't say its type.

In the code: crates/anyx-protocol/protocol.grammar.anyx (the `TYPE` declaration). Issues #51, #57.
