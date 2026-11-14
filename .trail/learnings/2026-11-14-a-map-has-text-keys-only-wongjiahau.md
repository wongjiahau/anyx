---
title: "A MAP has text keys only, written as strings in braces, and may hold its own type"
author: Wongjiahau
date: 2026-11-14
branch: master
cost: { minutes: 30, tokens: 121102 }
id: l-6754cdd8
scope: repo
status: active
src: agent
touches:
  - crates/anyx-protocol-generate/README.md
  - crates/anyx-protocol-generate/src/rust.rs
  - crates/anyx-protocol-generate/src/typescript.rs
  - crates/anyx-protocol-generate/tests/fixtures/types.anyx
  - crates/anyx-protocol-generate/tests/generated/types.rs
  - crates/anyx-protocol-generate/tests/generated/types.ts
  - crates/anyx-protocol-generate/tests/types_round_trip.rs
  - crates/anyx-protocol/PROTOCOL.md
  - crates/anyx-protocol/protocol.anyx
  - crates/anyx-protocol/src/check.rs
---

## Decided
`MAP: T` maps text keys to `T`. On the wire it is braces of entries, each a string key and its value: `{ "hymn" 3, "ode" 1 }`, with `"hymn": done` when the value is words, the same `:` rule as record fields. Keys are always quoted, never raw strings; each key once; written in key order (BTreeMap). JSON: an object whose keys are kept as written, not camelCased. Rust `BTreeMap<String, T>`, TypeScript `Record<string, T>`. A type may hold itself through a `MAP` as through a `LIST`, since a map may be empty. `MAP: T DEFAULT {}` and `MAP name DEFAULT {}` work like the `LIST` forms.

## Tried and ruled out
- Bare-word keys (`{ hymn 3 }`): they would look like record fields and words join the call name, so `{ old ode 3 }` couldn't tell key from value. Strings make any text a key.

## Watch out
- An entry parses as the call `_ _` of a string and the value; `"tea" 5 minutes` is `_ _ minutes`, so a words value needs the `:`.
- `Record`, `Date` and `Reviver` are now reserved type names, since a schema type with that name would shadow TypeScript's or the client's.

In the code:
- crates/anyx-protocol/protocol.anyx (`type` and `defaulted` gained `MAP`), src/codec.rs (`decode_entries`, `write_quoted`), src/json.rs, src/check.rs (`check_recursion` message)
- crates/anyx-protocol-generate/src/typescript.rs (`Revivers::of` for maps), src/lib.rs (`RESERVED_TYPE_NAMES`)
