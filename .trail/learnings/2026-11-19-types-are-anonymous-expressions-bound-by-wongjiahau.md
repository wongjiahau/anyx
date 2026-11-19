---
title: "Types are anonymous expressions, bound by an alias or a named type whose constructor is its shape"
author: Wongjiahau
date: 2026-11-19
branch: feat/fd-protocol-tool
cost: { minutes: 1, tokens: 2684 }
id: l-32bb5f5b
scope: repo
status: active
src: agent
---

## Decided
The protocol's type system:

- Type expressions are anonymous: `Integer`, `LIST X`, `MAP X`,
  `X WHERE [...]`, `RECORD { ... }`, `CHOICE { ... }`.
- `TYPE ALIAS Info: Integer WHERE [_ > 10]` is transparent: `15` is an
  `Info`, no constructor.
- `NAMED TYPE` is nominal, and its declaration is a shape whose slots hold
  type expressions; its constructor is that same shape with values in those
  slots, so declaration and value read alike:
  `NAMED TYPE (CHOICE { red, blue }) Color` is written `red . Color`;
  `NAMED TYPE Info (Integer WHERE [_ > 10])` is written `Info 15`.
- `CHOICE X { ... }` and `RECORD X { ... }` stay, as shorthand for
  `TYPE ALIAS X: CHOICE { ... }` and `TYPE ALIAS X: RECORD { ... }`, not
  for `NAMED TYPE`: their values keep no constructor (`{ hidden }`,
  `newest first`), so records and choices are structural. A `NAMED TYPE`
  is how a name shows at the call site: `NAMED TYPE (CHOICE { red, blue })
  Color` is written `red . Color`.
- A named type may have several type-expression slots; its value is then a
  tuple, as a record field with several slots is:
  `NAMED TYPE Range from (Integer) to (Integer)`, written `Range from 1 to 3`.
- Type parameters stay in the name's slots, written `(item: Type)`, and a
  value fills them like any slot: no inference, no omitting.
  `NAMED TYPE Page of (item: Type) (RECORD { items: LIST item })` is written
  `Page of (Text) { items ["a"] }`; the type is `Page of (Text)`. The
  shorthand `RECORD Page of (item: Type) { ... }` is an alias, so its value
  is `{ items ["a"] }`, checked against the expected type.

Why: a nominal type written as plain text on the wire, as `TYPE File Id:
Text` was, doesn't show at the call site which type a value is (the shell
needed `AS:` for that). A value written with its constructor means exactly
what it shows.
