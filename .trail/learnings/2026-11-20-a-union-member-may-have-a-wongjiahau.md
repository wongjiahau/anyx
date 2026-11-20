---
title: "A union member may have a doc before it, as a record field does"
author: Wongjiahau
date: 2026-11-20
branch: feat/variant-backtick
cost: { minutes: 41, tokens: 226073 }
id: l-9ab7da26
scope: repo
status: active
src: agent
touches:
  - crates/anyx-protocol-generate/src/typescript_native.ts
  - crates/anyx-protocol-generate/tests/generated/generic.ts
  - crates/anyx-protocol-generate/tests/generated/named.ts
  - crates/anyx-protocol-generate/tests/generated/overloaded.ts
  - crates/anyx-protocol-generate/tests/generated/poentaro.ts
  - crates/anyx-protocol-generate/tests/generated/refined.ts
  - crates/anyx-protocol-generate/tests/generated/scoped.ts
  - crates/anyx-protocol-generate/tests/generated/types.ts
  - crates/anyx-fd/README.md
  - crates/anyx-fd/fd.protocol.anyx
---

## Decided
- A `UNION [ … ]` member may have a `#` line string before it, as a record field does, and it applies to every member, not only variants. So when `CHOICE` becomes a union of variants (#58), the docs on its variants carry over to the guide and the generated code:
  TYPE ALIAS Case: UNION [
      # Any case, unless the pattern has an uppercase letter.
      `smart case,
      `any case,
  ],

## Tried and ruled out
- Keeping docs only on `CHOICE` variants: `CHOICE` goes away in #58, and dropping the docs would lose them from fd's schema, the guide and generated code.

In the code: the `union member` rule in crates/anyx-protocol/protocol.grammar.anyx gains an optional LINE STRING doc.
