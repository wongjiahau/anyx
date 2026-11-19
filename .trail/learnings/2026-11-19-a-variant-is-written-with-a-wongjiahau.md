---
title: "A variant is written with a backtick, and a named type's constructor is a function"
author: Wongjiahau
date: 2026-11-19
branch: feat/named-type
cost: { minutes: 37, tokens: 442523 }
id: l-e7e4a831
scope: repo
status: active
src: agent
touches:
  - crates/anyx-fd/fd.protocol.anyx
  - crates/anyx-protocol-generate/README.md
  - crates/anyx-protocol-generate/src/lib.rs
  - crates/anyx-protocol-generate/src/rust.rs
  - crates/anyx-protocol-generate/src/typescript.rs
  - crates/anyx-protocol-generate/src/typescript_native.ts
  - crates/anyx-protocol-generate/tests/fixtures/generic.protocol.anyx
  - crates/anyx-protocol-generate/tests/fixtures/named.protocol.anyx
  - crates/anyx-protocol-generate/tests/fixtures/native.protocol.anyx
  - crates/anyx-protocol-generate/tests/fixtures/overloaded.protocol.anyx
---

## Decided
- At a use site a variant is written with a backtick: `` `red ``, `` `true ``, `` `null ``. Anything written without one is a function call: a prelude FUNCTION (`_ length`, Duration's `1 hours 30 minutes`) or a named type's constructor (`File Id "a"`).
- A named type's constructor is a function, not a variant: it can refuse its argument (a WHERE on its slot), and its result is a value of the named type. Its JSON tag (`{"FileId": "a"}`) names the function that made the value.
- Variants carry the mark, not functions, so the most-read code stays clean: ids and durations in requests, and predicates, where every call is a function. The cost is `` `true ``, `` `false `` and `` `null ``.
- A variant's declaration carries the backtick too: `` UNION [File Id, `red, `missing (id: File Id)] ``, `` ERRORS [`missing (id: File Id)] ``. Once CHOICE becomes a UNION of variants (#58), no keyword says which members are variants, so the mark is what tells a variant from a type. A declaration and its uses then read the same.
- A symbolic variant takes a parenthesis: `` `(1 + 1) `` is the variant `_ + _` sent as written, while `(1 + 1)` is the prelude function, evaluated to 2. Without the parenthesis, `` `1 + 1 `` is the flat shape `` ` _ + _ ``, which is never regrouped.
- `` ` `` is reserved in the protocol. The generated parser already reads `` `red `` as the call `` ` _ `` around `red`, so the grammar stays context-free and nothing is parsed on top of the tree. The checker reads that call as the variant mark. Declaring `` ` `` in any shape (function, variant, type) is an error.
- A shape is unique within its kind only: the variant `` `(_ + _) `` and the prelude function `_ + _` may both exist, since the mark keeps them apart. The mark gives the checker one table per call (variants of the expected type, or functions by shape then argument types), so an error names the one thing that was missing instead of guessing between a variant and a function.
- Only values cross the wire: literals, lists, maps, records and variants. A function call is evaluated by the sender. A named type's value is written as its constructor call, `File Id "a"`, since the constructor only checks and never changes its argument, so each value has exactly one call. A Duration has many calls for one value, so the encoder writes one fixed form.

## Tried and ruled out
- No mark at all, resolving by lookup: works without inference, but a reader can't see whether a call is data kept as written or a value computed by the codec.
- Marking functions: puts the backtick on every id in a request and every call in a predicate.
- Marking functions only inside values: one function would be written two ways.

## Watch out
- A marked variant in an argument needs parentheses: `` paint (`red) if (`true) ``. `` ` `` is a symbol word, so `` paint (`red) if `true `` is the flat shape `` _ ` _ ``. After a keyword it stands alone: `` ERROR `missing (…) ``, `` RETURNS `red ``.
- OCaml's backtick exists for a different reason (an undeclared tag whose type is inferred). Here every variant is declared; the mark is for the reader.

In the code: crates/anyx/src/lexer.rs (`` ` `` is a symbol character), crates/anyx-protocol/src/annotation.rs (`constructed_by`), crates/anyx-protocol/prelude.protocol.anyx.
