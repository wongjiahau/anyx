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
- (Superseded for `CHOICE`, below.) `CHOICE X { ... }` and `RECORD X { ... }` stay, as shorthand for
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

- Type parameters are Title Case, `(Item: Type)`; one sharing a name with
  a type in scope is an error.
- Recursion must go through a `NAMED TYPE`: a recursive alias would expand
  forever. `RECORD` and `CHOICE` are aliases, so this holds for them too.
- `UNION [A, B]` is an untagged union: a value of a member is a value of the
  union, written as that member writes it. No two members may share a
  written form (the `@` field rule), so decoding is a lookup and a subtype
  check against a union never tries several members.
- `T?` is `TYPE ALIAS (Item: Type) ? : UNION [Item, Null]` in the prelude,
  with `NAMED TYPE Null`, a unit type whose only value is `Null`. So
  `max depth 3` is honest, not sugar, and `value _` goes. The user hated
  writing `value`.
- Subtyping stays small: `T <: UNION [T, U]`, named types by name,
  refinements by base and identical predicate, records and choices by
  structure, no intersections or inference.

- A union member must be a named type, an intrinsic, or a refinement of
  one (written as its base). Records, lists, maps, choices and unions must
  be named first. So only intrinsics can clash (Integer/Float, Text/Instant,
  two refinements of one base), and `Depth??` is rejected where declared.
  A type parameter in a union is checked where it is filled.
- Refinements are anonymous: `TYPE ALIAS Depth: Integer WHERE [0 <= _]`;
  `Depth` and `Size` with the same predicate are one type.
- JSON always tags a named type's value; `Null` is plain `null`.
- A record with fewer fields fits where more are expected if each missing
  one has a `DEFAULT`; extra fields never fit.
- One generated enum per `CHOICE`, even for identical ones.
- Renaming a named type breaks the wire; accepted while unstable.

- `CHOICE` goes. Case tells what a word is: lowercase words are a variant,
  which is a type of its own; Title Case is a type, a named type's
  constructor or a type parameter; all caps is a keyword. A choice is
  `UNION [smart, any, exact]`, as with OCaml's polymorphic variants but
  marked by case, not a backtick.
- A union member is a variant, a named type, an intrinsic or refinement,
  or another union. `UNION []` is allowed, the identity. Forms may not
  repeat anywhere in the nesting, in anyx or in JSON; repeats are errors,
  never merged.
- Unions are associative and commutative: the checker normalizes a union
  into a flat set keyed by form, stopping at named types, intrinsics and
  variants. Generated code keeps a named alias member nested
  (`Option<Shade>`), flattens inline members.
- `ERRORS [File Errors, too large]` takes a list of union members; absent
  is `ERRORS []`. No spread marker: case tells a group from a variant. The
  `ERRORS` declaration and `+` go.
- `Boolean` is `TYPE ALIAS Boolean: UNION [false, true]`. `Boolean?` is
  allowed (banning it would special-case one alias); `PROTOCOL.md` advises
  named variants for a third state.
- JSON tags `Duration` and `Instant` like named types:
  `{"Duration": 3600000}`, `{"Instant": "…"}`.
- Lists, maps, `?` and generic slots are covariant. A record with extra
  fields never fits a smaller one.

Ruled out: a spread marker (`...File Errors`) in `ERRORS`, since `.` is
the grouping operator and case already tells a type from a variant.
Banning `Boolean?`. Flattening nested unions so `Depth??` is `Depth?` (a smart
coercion; the user declined). `A | B | C` for unions, since three members make the flat call
`_ | _ | _`; a list keeps one call for any count. Lowercase type parameters.

Tracked in https://github.com/wongjiahau/anyx/issues/51.

Why: a nominal type written as plain text on the wire, as `TYPE File Id:
Text` was, doesn't show at the call site which type a value is (the shell
needed `AS:` for that). A value written with its constructor means exactly
what it shows.
