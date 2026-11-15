---
title: "A number's type is how it is written: `3` is an INTEGER, `3.0` a FLOAT (NUMBER renamed to FLOAT)"
author: Wongjiahau
date: 2026-11-15
branch: work/overloading
id: l-7d3a9e21
scope: repo
status: active
src: agent
touches:
  - crates/anyx-protocol/protocol.anyx
  - crates/anyx-protocol/PROTOCOL.md
  - crates/anyx-protocol/src/codec.rs
  - crates/anyx-protocol/src/json.rs
  - crates/anyx-protocol/src/guide.rs
---

## Decided
- `NUMBER` is now `FLOAT` (issue #22): a finite 64-bit IEEE 754 float, `f64` in Rust, `number` in TypeScript. Only the name changed. `Type::Float` and `Value::Float` in the code.
- As in Rust, a number literal states its own type: `3` and `-1` are `INTEGER`s; `3.0`, `1.5`, `1e3` are `FLOAT`s. `codec::is_float_text` decides it from the text the lexer keeps (`NodeKind::Number(text)`): a `.` or an exponent, and not a `0x`/`0b`/`0o` number. So a number never needs an `AS:` annotation, and an INTEGER/FLOAT overload (#19) is always decidable on the wire.
- An INTEGER where a FLOAT goes: "found the INTEGER `3`; expected a FLOAT, such as `3.0`". A FLOAT where an INTEGER goes: "found the FLOAT `1.5`; expected an INTEGER, a whole number written without a point, such as `3`". Schema `DEFAULT`s and `EXAMPLES` go through `codec::decode`, so they get the same errors.
- A FLOAT that parses to infinity (`1e999`) is rejected when read, not only when written.
- JSON can't tell `3` from `3.0`, so the JSON path takes a number's type from the parameter or field it fills: `3` for a FLOAT is `3.0`. The codec writes a FLOAT with `{:?}`, which always has a point or an exponent.
- No 32-bit float (the wire is decimal text; narrowing is the server's job, a range is a refinement, #11) and no `DECIMAL` yet (money is INTEGER cents).

## Watch out
- `impl FromValue for f64` still accepts a `Value::Integer`; it is a lenient conversion in app code, not on the wire.
- `version: NUMBER [INTEGER, …]` in protocol.anyx is the grammar language's `NUMBER` pattern, not the protocol type. Don't rename it.

In the code: `is_float_text` and the `Type::Integer`/`Type::Float` arms of `codec::decode`; tests in crates/anyx-protocol/tests/numbers.rs.
