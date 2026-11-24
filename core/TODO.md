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

3. Turn the generated tree into core terms, consuming the tree as it is. (done)
   - Resolve exact call names in one file-wide namespace and resolve locals
     lexically. This lets declarations refer to one another in any order.
   - Reject duplicate names in the same scope and allow nested bindings to
     shadow outer ones. Each scope has one meaning for each name.
   - Reserve `Type` so it always denotes the universe.
   - Give constructors their own explicit parameters so any arguments to the
     data type are written in the constructor's signature.
   - In patterns, resolve a bare constructor name to that constructor and
     bind other bare names within the case. This needs no type information.
   - Abstract a call's direct holes from left to right and reject holes outside
     calls. The generated tree retains call nesting and argument order.
   - Resolve parameter annotations in written order, with earlier parameters
     in scope. This makes dependencies visible and rules out cycles.
   - Keep resolved binding identities in the lowered tree so item 4 can
     choose the kernel's indexing and evaluation representation.

## Kernel

4. Term representation: de Bruijn indices for terms, separate values for (done)
   evaluation.
   - Convert resolved binding identities in a separate pass so syntax and name resolution keep their existing interface.
   - Count indices from the innermost binder and value levels from the outermost so local values stay stable when a scope grows.
   - Keep declaration parameters in written order and place pattern binders into scope left to right so each index has a predictable meaning.
   - Drop local binder names in kernel terms because indices carry their identity, and retain exact global names for lookup.
   - Give function values closures over ordered environments and represent blocked eliminations as neutral values so evaluation can retain their inputs.
   - Carry match and literal forms through to the kernel so conversion preserves the lowered tree.
5. Evaluation, read-back and conversion checking, by normalisation by evaluation. (done)
   - Keep global function bodies opaque until item 7 so normalization does
     not evaluate unchecked recursive declarations.
   - Reduce matches in case order and retain blocked matches with their
     environments so read-back can normalize branches under fresh binders.
   - Keep constructor names and arguments together in values so patterns can
     inspect them without unfolding global definitions.
   - Compare beta-normal forms structurally, using decoded text and a single
     application spine per head, so source spelling and call grouping do not
     affect conversion. Leave type-directed eta rules for the type checker.
6. Bidirectional checking of `Type`, function types, holes, calls and names. (done)
   `Type` is in `Type` for now: a known inconsistency.
   - Supply closed global types to the checker because declaration checking starts in item 7.
   - Check hole-created functions against an expected function type because their parameter types must come from written annotations.
   - Compare types by beta normalization because it reuses the kernel's existing conversion rule.
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
