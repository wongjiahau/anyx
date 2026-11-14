---
title: "A DEFAULT goes only on an OPTIONAL record field, and the reader fills it in"
author: Wongjiahau
date: 2026-11-14
branch: master
cost: { minutes: 30, tokens: 121102 }
id: l-4cdc1bb9
scope: repo
status: active
src: agent
touches:
  - crates/anyx-protocol-generate/README.md
  - crates/anyx-protocol-generate/src/rust.rs
  - crates/anyx-protocol-generate/src/typescript.rs
  - crates/anyx-protocol-generate/tests/fixtures/types.anyx
  - crates/anyx-protocol-generate/tests/generated.rs
  - crates/anyx-protocol-generate/tests/generated/poentaro.ts
  - crates/anyx-protocol-generate/tests/generated/scoped.ts
  - crates/anyx-protocol-generate/tests/generated/types.rs
  - crates/anyx-protocol-generate/tests/generated/types.ts
  - crates/anyx-protocol-generate/tests/types_round_trip.rs
---

## Decided
`DEFAULT value` is allowed only after the type of an `OPTIONAL` field of a `RECORD`: `limit: OPTIONAL: INTEGER DEFAULT 50`. The value is written as on the wire and decoded against the field's type by the checker, so a wrong default is a schema diagnostic. The codec and the JSON reader fill the default in when the field is left out (JSON: also when it is `null`), so generated Rust and TypeScript types make a defaulted field plain (`i64`, `number`), not `Option`/`?`. A default on a required field or on a parameter is an error with a hint, since a parameter is always written.

## Tried and ruled out
- Parsing `INTEGER DEFAULT 50` as type plus suffix: all-caps words side by side join into one call name, so `INTEGER DEFAULT _` is a single call. The grammar's `defaulted` rule lists each built-in keyword with `DEFAULT` (`TEXT DEFAULT _`, ...), plus `LIST` nesting so `LIST: TEXT DEFAULT []` gives the whole field the default, `LIST name DEFAULT [...]`, and `(type) DEFAULT _` for anything else.
- Defaults on parameters: a call always writes every slot, so there is nothing to fill.

## Watch out
- `:` is right-associative, so in `OPTIONAL: LIST: TEXT DEFAULT []` the `DEFAULT` belongs to the innermost `TEXT` syntactically; the checker lifts it to the field.
- Defaults that name a `RECORD` or `CHOICE` are read after all types are known, in repeated passes, because a default can mention a type defined later.
- A new built-in type keyword needs a `defaulted` arm in the grammar too.

In the code:
- crates/anyx-protocol/protocol.anyx (`field type`, `defaulted`), src/grammar.rs regenerated
- crates/anyx-protocol/src/check.rs (`PendingDefault`, `read_defaults`), src/schema.rs (`Field.default`, `default_outside_record`)
- crates/anyx-protocol/src/codec.rs (`decode_fields`), src/json.rs (`fields_from_json`)
- crates/anyx-protocol-generate/src/rust.rs (`may_be_missing`), src/typescript.rs
