---
title: INSTANT is RFC 3339 text in UTC and DURATION a run of numbers with units; both keep milliseconds
author: Wongjiahau
date: 2026-11-14
branch: master
cost: { minutes: 30, tokens: 121102 }
id: l-246d71df
scope: repo
status: active
src: agent
touches:
  - TODO.md
  - crates/anyx-protocol/PROTOCOL.md
  - crates/anyx-protocol/tests/schema.rs
  - crates/anyx-protocol-generate/src/typescript.rs
  - crates/anyx-protocol-generate/tests/generated/poentaro.ts
  - crates/anyx-protocol-generate/tests/generated/scoped.ts
  - crates/anyx-protocol-generate/README.md
  - crates/anyx-protocol-generate/src/wasm.rs
  - crates/anyx-protocol-generate/tests/generated/poentaro_wasm.rs
  - crates/anyx-protocol/src/json.rs
---

## Decided
- `INSTANT` is a string on the wire and in JSON: `"2026-11-14T06:00:08.000Z"`, always UTC (`Z`), uppercase `T` and `Z`, years 0000 to 9999. It is written with exactly three fraction digits and read with none or one to three; a fourth digit, an offset like `+08:00` or a lowercase `t`/`z` is rejected.
- `DURATION` is words on the wire: `1 hour 30 minutes` is the anyx call `_ hour _ minutes` of `1` and `30`, so it goes in parentheses in a slot (`wait (5 minutes)`) and after `:` in a record (`{ every: 1 hour 30 minutes }`), like a variant. Units are days, hours, minutes, seconds, milliseconds, singular or plural. A count is a whole number from 0; each unit at most once, largest first. Read accepts `90 seconds`; write is canonical (`1 minute 30 seconds`, `0 seconds` when empty, singular for 1).
- JSON carries a `DURATION` as whole milliseconds (a number), an `INSTANT` as its text.
- Both keep milliseconds, rounding finer time down (an instant before the epoch rounds toward the past). `Value::Instant(SystemTime)` and `Value::Duration(Duration)` hold the exact value; only the wire drops precision.
- Rust: `::std::time::SystemTime` and `::std::time::Duration`, written with their full path so a schema type named `instant` or `duration` can't clash. No date crate: `src/time.rs` converts civil dates with Howard Hinnant's days-from-civil algorithm.
- TypeScript: `Date` and `number`. JSON.parse gives text, so the generator writes a `revive<Type>` function for each type, list and method error union that holds an `INSTANT` (found by a fixpoint over the types, since types can hold each other), and `call` takes the value's and the error's reviver. `JSON.stringify(date)` already writes the canonical text, so arguments need nothing.

## Tried and ruled out
- ISO 8601 durations (`"PT1H30M"`): the words read better to an agent and fit how anyx tokenizes `5 minutes`.
- A JSON.parse reviver that turns any date-like string into a `Date`: it would also turn `TEXT` that happens to look like a date.

## Watch out
- A record field whose value is a duration needs the `:`: `every 5 minutes` reads as the call `every _ minutes`, not a field.
- The generated TypeScript was checked with `tsc --strict --skipLibCheck` by hand; no test runs `tsc`.

In the code: crates/anyx-protocol/src/time.rs; `decode` and `write` in crates/anyx-protocol/src/codec.rs; crates/anyx-protocol/src/json.rs; `Revivers` in crates/anyx-protocol-generate/src/typescript.rs; crates/anyx-protocol-generate/tests/fixtures/types.anyx and tests/types_round_trip.rs.
