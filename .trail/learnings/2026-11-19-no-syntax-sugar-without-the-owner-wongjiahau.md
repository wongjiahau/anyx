---
title: "No syntax sugar without the owner's approval"
author: Wongjiahau
date: 2026-11-19
branch: feat/fd-protocol-tool
cost: { minutes: 17, tokens: 181625 }
standing: true
id: l-0b25366f
kind: guardrail
scope: repo
status: active
src: agent
touches:
  - TODO.md
  - crates/anyx-fd/fd.protocol.anyx
  - crates/anyx-protocol/PROTOCOL.md
  - crates/anyx-protocol/protocol.grammar.anyx
  - crates/anyx-protocol/src/check.rs
  - crates/anyx-protocol/src/grammar.rs
  - crates/anyx-protocol/tests/records.rs
  - crates/anyx-protocol/tests/snapshots/quoted_doc_diagnostics.txt
  - README.md
  - crates/anyx-fd/README.md
---

## Decided
No syntax sugar or implicit coercion goes into anyx, the protocol, or any
language built on them without the owner's explicit approval. Every form is
written as the call tree it is. When a form feels verbose, propose the
shortcut as a question with an example and wait for a yes.

## Tried and ruled out
A record field of a nullable type could be written as its item:
`at least 14 bytes` decoded to `value 14` for a `Size?` slot, and the encoder
stripped `value _` back off. It hid the sum type's constructor, made `14` mean
different things by position (field or argument), and spawned special cases:
`DEFAULT null` required on a nullable `@` field, skipping a null field on
encode, `to null` versus `to (null)`, and a per-slot `item` in the TypeScript
descriptors. It is being removed: a nullable field is written
`at least (value 14) bytes`.

## Watch out
Readability over writability (AGENTS.md): extra words that keep the tree
honest cost only writability.

In the code: `field_value` in crates/anyx-protocol/src/request.rs, `as_item`
in crates/anyx-protocol/src/codec.rs, `encodeSlot`/`decodeSlot` and `item` in
crates/anyx-protocol-generate/src/typescript_native.ts.
