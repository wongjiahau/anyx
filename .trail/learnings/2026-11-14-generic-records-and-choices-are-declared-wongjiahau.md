---
title: Generic records and choices are declared with GENERIC and used with WITH and named arguments
author: Wongjiahau
date: 2026-11-14
branch: master
cost: { minutes: 30, tokens: 121102 }
id: l-b5b3a5cc
scope: repo
status: superseded
src: agent
touches:
  - crates/anyx-protocol-generate/README.md
  - crates/anyx-protocol-generate/src/lib.rs
  - crates/anyx-protocol-generate/src/rust.rs
  - crates/anyx-protocol-generate/src/typescript.rs
  - crates/anyx-protocol-generate/tests/generated/poentaro.rs
  - crates/anyx-protocol-generate/tests/generated/poentaro.ts
  - crates/anyx-protocol-generate/tests/generated/poentaro_wasm.rs
  - crates/anyx-protocol/PROTOCOL.md
  - crates/anyx-protocol/protocol.anyx
  - crates/anyx-protocol/src/check.rs
---

> Superseded on 2026-11-15: `GENERIC` and `WITH` are being replaced by anyfix type shapes. See the learning "A generic type's name is an anyfix shape whose type parameters can sit anywhere". The checker rules and generated-code notes below still apply.

## Decided

A RECORD or CHOICE can be generic: `GENERIC (item) RECORD page { items: LIST: item }`, and `PUBLIC GENERIC (…) RECORD` to share one. A use names every argument, in any order: `page WITH (item: order)`, nested as `page WITH (item: outcome WITH (value: receipt, problem: decline))`. The wire format and JSON don't change: a value is written like the record or choice with the arguments in place of the parameters, and DEFAULTs and EXAMPLES are checked against the instantiated type.

Rules the checker enforces, beyond the ones asked for:
- A parameter no field or variant uses is an error, so Rust never sees an unused type parameter.
- A parameter can't share its name with a type visible where it is declared: a type of its scope, or any scope's PUBLIC type by its short name.
- Inside its own declaration a generic type uses itself only with its own parameters as they are (`tree WITH (item: item)`), and only through LIST or MAP. Any other self use is rejected, even a closed one such as `tree WITH (item: TEXT)`, which keeps the rule simple and the instantiations finite.
- After LIST, MAP or OPTIONAL a use needs the `:`, `LIST: page WITH (…)`, because keywords side by side join into one call; the decoder says so with a hint.

Generated code: real generics. Rust `pub struct Page<Item>` with `impl<Item: IntoValue> IntoValue for Page<Item>`; TypeScript `interface Page<Item>`. A parameter whose PascalCase name is already taken (a reserved name such as `Value`, a type of the scope in Rust, any type in TypeScript) gets `Parameter` appended: `ValueParameter`. A generic type's TypeScript reviver takes a reviver per parameter and returns its own; a use passes them, with `unchanged` for an argument that needs none.

## Tried and ruled out

- `page OF item`, from the issue: domain-specific English that reads well for one parameter and badly for several, and positional.
- `FOR ALL (item) RECORD page`: too mathematical for the people writing schemas.
- Positional arguments, `page WITH (order)`: named arguments say what each type is for, and the order can't be mixed up.
- Fully qualified paths in generated Rust so any parameter name works: more churn in every snapshot than renaming the few clashing parameters.

## Watch out

- anyx's canonical formatter drops a redundant paren, so `format_schema` writes `GENERIC item RECORD page` and `page WITH item: order` for one parameter or argument; several keep their parentheses. This is the formatter's existing behaviour, as with `RETURNS x: INTEGER`.
- `Type::Generic(index, arguments)` keeps the arguments in the order of the parameters, not as written. Read a record or variant's fields through `Schema::instance`, which substitutes the arguments; matching `Type::Named` alone misses generic uses.
- A missing argument is reported and filled with TEXT internally so checking can go on.

In the code:
- Grammar: `crates/anyx-protocol/protocol.anyx` (`type parameters`, `type arguments`, the `generic` forms of `record`, `choice`, `type`, `defaulted`); regenerate with `UPDATE_SNAPSHOTS=1 cargo test -p anyx-protocol --test schema the_schema_decoder`.
- Checking: `generic`, `generic_use`, `check_parameters_used` and `check_generic_recursion` in `crates/anyx-protocol/src/check.rs`.
- Model: `Type::Generic`, `Type::Parameter`, `Type::substitute`, `Schema::instance` in `crates/anyx-protocol/src/schema.rs`.
- Generators: `Generics` in `crates/anyx-protocol-generate/src/rust.rs`, `reviver` and `Revivers::of` in `typescript.rs`, `type_parameter_name` in `lib.rs`.
- Tests: `crates/anyx-protocol/tests/generics.rs`, `tests/snapshots/generic_diagnostics.txt`, `crates/anyx-protocol-generate/tests/fixtures/generic.anyx` and `generic_round_trip.rs`.
