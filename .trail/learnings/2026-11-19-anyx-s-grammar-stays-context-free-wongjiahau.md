---
title: "anyx's grammar stays context-free, so custom brackets are found by the lexer alone"
author: Wongjiahau
date: 2026-11-19
branch: feat/fd-protocol-tool
cost: { minutes: 1, tokens: 11604 }
id: l-9674af03
scope: repo
status: active
src: agent
touches:
  - TODO.md
  - crates/anyx-fd/README.md
  - crates/anyx-fd/fd.protocol.anyx
  - crates/anyx-fd/tests/fd.rs
  - crates/anyx-protocol-generate/src/rust.rs
  - crates/anyx-protocol-generate/src/typescript.rs
  - crates/anyx-protocol-generate/src/typescript_native.ts
  - crates/anyx-protocol-generate/tests/fixtures/generic.protocol.anyx
  - crates/anyx-protocol-generate/tests/fixtures/named.protocol.anyx
  - crates/anyx-protocol-generate/tests/fixtures/refined.protocol.anyx
---

## Decided
- anyx's grammar stays context-free. A token's kind never depends on which brackets are open.
- For custom brackets (#53), this means:
  - `[| ` (a bracket, then symbols, then whitespace) is always an opener.
  - ` |]` (whitespace, then symbols, then a closing bracket) is always a closer.
  - The parser matches the two as it matches `(` with `)`.
  - The whole run of symbols must match, and a closer is never split.

## Tried and ruled out
- A closer that depends on the innermost open bracket, so that ` |]` is a closer only inside `[|`. It needs a bracket stack in the lexer, which can disagree with the parser's error recovery, or else the parser has to split a token back into a word plus `]`.

## Watch out
- An ordinary bracket can't end with a symbol word after a space: `[a |]` is the closer `|]` and an error. Write `[a | ]`.

In the code: crates/anyx/src/lexer.rs (`Lexer` holds no stack; `token`), crates/anyx/src/parser.rs (bracket matching around line 52).
