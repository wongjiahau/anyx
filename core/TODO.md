# The core: what to build, in order

The first version is the smallest core: `Type`, dependent function types
(`(x: a) RETURNS b`), holes as anonymous functions, calls, `FUNCTION`, `DATA`
and `MATCH`. Every argument is written out: no `GIVEN`, no type classes, no
inference. Macros and effects come later.

## Syntax

1. Remove `MACRO`, `QUOTE`, `UNQUOTE`, the macro-use catch-all and `GIVEN` (done)
   from `core.grammar.anyx`. That also removes both generator warnings.
2. Create a `crates/anyx-core` crate whose parser is generated from that (done)
   grammar, as the other crates do.
   - Check in the generated parser and expose it as `anyx_core::parser` so callers can use the typed tree directly and builds need no generator.

## Front end

3. Turn the generated tree into core terms, consuming the tree as it is.
   - Name resolution: variables, functions and constructors.
   - Holes as functions: decide how far a hole's function reaches (the
     nearest parentheses?).
   - Parameter order: decide whether a name's parameters may come in any
     order without cycles, as in `(x: a) equals (y: a) in (a: Type)`.

## Kernel

4. Term representation: de Bruijn indices for terms, separate values for
   evaluation.
5. Evaluation, read-back and conversion checking, by normalisation by
   evaluation.
6. Bidirectional checking of `Type`, function types, holes, calls and names.
   `Type` is in `Type` for now: a known inconsistency.
7. `FUNCTION` declarations, including recursion between them.
8. `DATA` declarations: constructors must return their own type, and strict
   positivity holds.
9. `MATCH` on plain data, with exhaustiveness and redundancy checks.
10. `MATCH` on indexed families such as `Vector`, with index unification.
11. Termination: structural recursion only, at first.

## Usable

12. Literals: decide whether `NUMBER` and `STRING` stay, with built-in
    `Natural` and `Text`, or wait for a prelude.
13. Errors with source spans, and a CLI that checks a file and evaluates a
    chosen expression.
14. Tests: the sample declarations as passing cases, and a failing case for
    each check.

## Later

- Macros: `MACRO`, `QUOTE`, `UNQUOTE`, with a call whose name holds a keyword
  being a macro use.
- Effects: monads as library types, or effect rows.
- Universe levels, and whether `MATCH` stays in the core or becomes
  recursors.
