---
title: A named TYPE is nominal in generated code and invisible on the wire
author: Wongjiahau
date: 2026-11-14
branch: work/named-types-and-examples
cost: { minutes: 30, tokens: 121102 }
id: l-f87b92a0
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
- `TYPE file id: TEXT` (and `PUBLIC TYPE`) is a distinct type, never an alias: `TypeKind::Distinct(Type)` in the checked schema. The codec, JSON and the guide's sample values read past it with `Schema::underlying`, so a `Value` of a named type is the value underneath and a response is unchanged by a `TYPE`. Since #19 a request annotates an argument of a named type, `open ("abc" AS: file id)`, so giving a parameter a named type changes its calls (see the learning on values with no type of their own).
- Named `TypeKind::Distinct`, not `TypeKind::Named`, because `Type::Named(index)` already means "a reference to a defined type"; two meanings of "named" side by side would mislead.
- Any type may go underneath (`LIST: TEXT`, a record, a choice, another named type), except one that leads back to the named type through other named types, even inside a `LIST` (`TYPE a: LIST: a`): `check_named_cycles` reports it with its own message. A record that holds a named type over itself is caught by the usual holding-itself check, where a named type holds the type underneath directly.
- Rust: `pub struct FileId(pub String)` with `From` both ways and `IntoValue`/`FromValue` delegating to the inner value. TypeScript: `type FileId = string & { readonly __brand: "file id" }` plus `function FileId(value: string): FileId` to brand a value; the brand is the full path (`common/file id`) so scopes stay distinct. The JSON from wasm needs no conversion, since a brand exists only at compile time.
- The guide lists each named type in its Values section as "`file id`: `TEXT`." with its doc on one line, only when the schema has any.

## Tried and ruled out
- A plain alias next to the distinct type: left out on purpose (the issue's open question), since an alias gives no safety.

## Watch out
- `WHERE [...]` refinements (#11) will extend the same form: `TYPE revision: INTEGER WHERE [1 <= _]` parses as `TYPE revision _` of the call `_ WHERE _`, because `:` is loosest, so the `named type body` rule only needs its `type` to accept that call.
- The TypeScript client revives `INSTANT`s into `Date`s. A named type gets no reviver of its own: `Revivers::of` uses the reviver of the type underneath, since a brand is only a type. A named type that holds an instant still marks the records and choices around it as needing reviving.
- A named-type cycle counts mentions through `MAP` as well as `LIST` (`TYPE a: MAP: a` is rejected).
- Adding the `named type` choice to `definition` needed the decoder regenerated before the checker matched on `ast::Definition::NamedType`.

In the code: `named type` in crates/anyx-protocol/protocol.anyx; `named_type`, `check_named_cycles` and `cycles` in crates/anyx-protocol/src/check.rs; `TypeKind::Distinct` and `Schema::underlying` in src/schema.rs; `named_types` in src/guide.rs; `newtype` in crates/anyx-protocol-generate/src/rust.rs and `branded` in src/typescript.rs; tests in crates/anyx-protocol/tests/named_types.rs and crates/anyx-protocol-generate/tests/named_round_trip.rs.
