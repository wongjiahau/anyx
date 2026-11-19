---
title: "The shell checks a method's REQUIRES before a tool runs, so a call the schema rules out never reaches a program"
author: Wongjiahau
date: 2026-11-18
branch: feat/fd-protocol-tool
cost: { minutes: 1, tokens: 1004 }
id: l-ac58a866
scope: repo
status: active
src: agent
touches:
  - crates/anyx-shell/src/compile.rs
  - crates/anyx-shell/src/lib.rs
---

## Decided
- The anyx shell checks a call's REQUIRES itself. If the arguments are all written out, it checks them while it validates the whole document, so nothing runs. If they come from other calls, it checks them just before the call runs, and a failure is `INVALID REQUEST` at that call. Before this, the shell only checked refinements, so a contradictory settings list reached the program and came back as a decoding failure, which the shell showed as `INTERNAL ERROR`.
- A REQUIRES failure names the method and the values sent, written as anyx values. The REQUIRES source is quoted only when the block has one test, since a whole block with a dozen tests buries the one that failed.
- For now, a path that isn't Unicode is a declared error, `(path) is not unicode`. The alternatives were `Bytes` or a lossy conversion. This is open: revisit when a tool needs to pass such paths along.
- `USE fd FROM COMMAND "p"` again with the same program re-reads the schema. A different program under a name already in use is an error. If a document fails validation, the tools its USE lines brought in are rolled back.

## Watch out
- A program's standard error is shown only when its USE line fails. Per-call failures are `INTERNAL ERROR` with no detail, because the shell has no log yet.

In the code: `check_preconditions` in crates/anyx-shell/src/compile.rs, `evaluate_call` in crates/anyx-shell/src/lib.rs, `anyx_protocol::refinement::check_precondition` (now public), `Interpreter::import` and `Staged`.
