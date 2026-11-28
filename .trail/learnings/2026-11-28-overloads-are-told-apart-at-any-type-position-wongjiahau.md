---
title: "Overloads are told apart at any type position, the result included"
author: Wongjiahau
date: 2026-11-28
id: l-7c2e4b91
scope: repo
status: active
src: agent
---

## Decided
- Two functions or constructors may share a name when, at some position (an argument or the result), their types are apart: both ground in normal form and different. Ground means no free variable, value variables like the `n` in `Vector of (n)` included, so a stuck type-family call never counts.
- `DATA Colour WITH [red, green]` and `DATA Light WITH [red, amber]` may both declare `red`. Constructors are functions, so they join the same overload sets, and so do records' constructors: `{ }` picks by its entries, then by the expected type.
- A call picks by its arguments, then by the type it's checked against. If neither settles it, it's an error, and the writer adds an ascription `(red : Colour)`.
- Each overload's own name writes every ground type, the result included, so adding an overload never renames another: `parse (Text) : Natural`.
- `:` replaces `RETURNS`, so a declaration reads the way an ascription does: `FUNCTION c : Colour IS red`.
- Why: a duplicate `red` across two types was an error, and that was too strict.

## Tried and ruled out
- **Naming overloads only by the positions that differ (`parse : Natural`):** this was shorter, but adding a third overload renamed the existing ones and broke generated clients.
- **Picking by the parameter's own name, or forbidding result-only overloads where another overload takes that type:** ascription is simpler and works anywhere.

## Watch out
- Soundness rests on two things:
  - picking only chooses a function, which the kernel checks as any application;
  - apartness needs normalizing to terminate, and different ground types to share no values. Subtyping between named types in the core would break this, since `Natural` and `Integer` would overlap.
- An overloaded call inside the argument of another overloaded call, at a position where the overloads are apart, has no expected type. It needs an ascription.

In the code:
- `core/TODO.md`, steps 27–29.
- `crates/anyx-core/src/kernel/checking.rs`, `check_overloads` and `resolve_overload`.
- `crates/anyx-core/src/lowering.rs`, `register_overload` and `overload_name`.
