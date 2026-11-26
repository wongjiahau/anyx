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
7. `FUNCTION` declarations, including recursion between them. (done)
   - Register all explicitly written function types, then check signatures before bodies so mutually recursive calls use their declared types in either order.
   - Check declaration bodies under their written parameters using the existing function checker so dependent binders keep their source order.
   - Keep function bodies opaque during conversion until termination checking is available so recursive calls cannot make type checking diverge.
   - Accept caller-supplied data and constructor types because checking `DATA` declarations belongs to item 8.
8. `DATA` declarations: constructors must return their own type, and strict (done)
   positivity holds.
   - Register all written signatures before checking them so data, constructors,
     and functions can refer to declarations in either order.
   - Require data signatures to end in `Type` and constructor results to
     name their fully applied data family so the target is explicit.
   - Allow recursive families as field types or in function results, and
     reject them in function domains, application arguments, and family
     parameter types because those positions have no established positivity.
   - Check cycles among data dependencies so mutually recursive declarations
     obey the same positivity rule as direct recursion.
9. `MATCH` on plain data, with exhaustiveness and redundancy checks. (done)
   - Match declared data types without parameters because indexed families need item 10.
   - Check each branch against the expected result type from the enclosing expression so no branch has to supply a result type.
   - Check constructor fields in written order so later field types can refer to earlier pattern values.
   - Check coverage over constructor trees because this detects overlap in nested patterns without trying to prove whether field types are inhabited.
   - Allow an empty case list for data without constructors because there are no constructor cases to write.
   - Report one missing pattern with fresh names for unconstrained fields so it can be pasted into a case.
10. `MATCH` on indexed families such as `Vector`, with index unification. (done)
    - Unify constructor result indices before checking nested fields, and keep refinements local to each case so dependent field and result types use the equations established by that pattern.
    - Unify local values and injective data and constructor heads, keeping blocked equations possible for coverage because opaque calls cannot establish impossibility.
    - Refine a local indexed subject to its matched constructor so a written return type can depend on the subject.
    - Inspect immediate constructors of untested indexed fields because index contradictions can prove those fields empty without an unbounded search for inhabitants.
11. Termination: structural recursion only, at first. (done)
    - Require every call in a recursive group to pass a constructor subfield at the same written parameter position because one strict descent suffices for direct and mutual recursion.
    - Check signatures first and publish each dependency group's bodies after type and termination checking because conversion must only run checked definitions.
    - Unfold recursive calls only when saturated with a constructor at the decreasing position because evaluation must stop on unknown inputs.
    - Refine local subjects in plain-data matches because inductive branches need the subject's constructor to compute their expected types.
    - Treat data occurrences hidden in transparent functions as nonpositive because a function can place them in a negative position.

## Usable

12. Literals: decide whether `NUMBER` and `STRING` stay, with built-in (done)
    `Natural` and `Text`, or wait for a prelude.
    - Defer `NUMBER` and `STRING` to a prelude so the core needs no built-in `Natural` or `Text` types.
13. Errors with source spans, and a CLI that checks a file and evaluates a (done)
    chosen expression.
    - Locate checking errors at the enclosing signature or body, and termination errors at the first implicated head, because the generated tree already records these spans.
    - Use `anyx-core FILE [--evaluate EXPRESSION --type TYPE]` because the expression and its expected type can be supplied explicitly and decoded with the existing grammar.
    - Print the normalized kernel term because the core has no surface syntax for every normal form.
14. Tests: the sample declarations as passing cases, and a failing case for (done)
    each check.
    - Run the sample through the CLI and exercise rejections in the existing test suites because each test can then assert the error at the layer that reports it.

## Universes

15. Two levels: `Type: Kind`, replacing `Type` in `Type`. (done)
    - `Kind` has no type: it may only be written as the result of a
      signature, never as a parameter's type or inside an expression.
    - A function type is in `Kind` when either side is, otherwise in `Type`:
      `(n: Natural) RETURNS Natural` is in `Type`, `(a: Type) RETURNS Type`
      is in `Kind`.
    - `DATA` keeps `RETURNS`, which must say `Type` or `Kind` (with any
      written parameters before it, as now). The checker verifies it and
      never infers it: a `DATA` that `RETURNS Type` may only have constructor
      fields in `Type`, so a value in `Type` never holds a type.
    - Tests: Girard's paradox is ruled out by rejecting a `Type` data type
      with a field `(item: Type)`, the same data accepted as `RETURNS Kind`,
      and a function that takes or returns a `Kind`-level type.

    - Allow a literal `Kind` as the final result of a `DATA` or
      `FUNCTION` declaration because `Kind` has no type of its own.
    - Keep inline `MATCH` annotations checked in `Type` because a match
      needs a fixed expected universe.

## Aliases

16. A constructor may name its family through an alias, a function that (done)
    returns a type: `get profile RETURNS Private endpoint returning (Profile)`.
    - Unfold the written result before the result and positivity checks so
      both see the family the alias stands for, with its indices.
    - Unfold only nonrecursive functions, because their bodies are not
      checked yet. A call can only name a global, so unfolding terminates.
    - Leave a result that does not reduce to the family, such as a `MATCH`
      blocked on a parameter, to be rejected as before.

17. A family's parameters don't count towards its universe, as in Coq (done)
    and Lean, so `List of (item: Type) RETURNS Type` and `List of (Text)`
    is a field of data in `Type`.
    - A constructor restates its family's arguments as binders. A binder of
      `Type` is exempt from "fields of data returning Type are in Type" only
      when it passes a parameter: a leading position where every
      constructor passes its own binder, in its result and in every
      recursive occurrence among its fields.
    - Anything else is an index or a stored field and stays checked, so
      `pack (code: Type) RETURNS Any` is still rejected (Girard's paradox).

18. A `FUNCTION` without `IS` is served: whatever serves the schema (done)
    implements it, and the checker treats it as an opaque constant.
    - Its result must be in `Type`, so a server never supplies a type.
    - It has no body, so it is outside termination checking and never
      unfolds.

19. `RECORD` is a `DATA` with one constructor, written as `{ }` with its (done)
    fields in any order. The kernel is unchanged: lowering builds the
    constructor call.
    - A field is a head whose every slot names its binder, as in `between
      (low: Natural) and (high: Natural)`, or a single `(case: Case)`, which
      is written as its value alone. An entry finds its field by the field's
      words or by the data type of its value. Two unnamed fields can't share
      a type.
    - The constructor's parameters are the record's own, then every field's
      binders in order. So a field's type can name an earlier field, and a
      value is checked in declaration order.
    - Each slot of a `DEFAULT` is a function without parameters, so it is
      type-checked and can't name another field. A field whose type names
      another field has no `DEFAULT`.
    - Generated code sends a record as its fields alone, without `kind`.
    - Not yet: projections (`length of (list)`), `{ }` for a record with
      parameters, and defaults on the wire.

20. `Natural` is built in, and `3` is a literal of it. (done)
    - The kernel checks `DATA Natural RETURNS Type WITH [zero RETURNS
      Natural, successor of (number: Natural) RETURNS Natural]` before every
      document, so a schema can't declare its own.
    - A closed `Natural` is one number: `zero` evaluates to `0` and
      `successor of` a number to the next one. A match or an index equation
      peels one step: `0` is `zero`, `n + 1` is `successor of (n)`. That rule
      is the only trusted code; there is no built-in arithmetic, so `(2) plus
      (3)` peels its left side twice.
    - Two different numbers never unify, without peeling down to `zero`.
    - Generated code sends a `Natural` as a number.
    - Not yet: literals of `Decimal`; built-in `plus`.

21. `Text`, `Integer` and `List of _` are built in, with literals. (done)
    - `"src"` is a `Text`, `-3` an `Integer`, and `3` a `Natural` or, where
      one is expected, an `Integer`.
    - `Text` and `Integer` are primitives: no constructors, and no `MATCH`
      takes them apart. Were they empty `DATA`, `MATCH "x" WITH []` would
      prove anything. Two literals are equal only when they are the same,
      and nothing reduces them.
    - `[a, b]` is a `List of (item)` when each element is an `item`. It is
      its own value, not a chain of `list of _ starting _ then _`, since
      it doesn't say its item type: a `MATCH` on it waits, and it is not
      convertible with the chain. That costs completeness, not soundness.
    - A schema can't declare any of the four.

## Later

- Overloading on named types without arguments, then record projections.
  - Overloads share a shape, such as `id of _`, and differ in at least one
    position whose parameter type is a name with no arguments (`Text`,
    `Key`), anywhere in the call.
  - A call picks the one overload whose names match its arguments'
    synthesized, normalized types: no backtracking, no unification.
  - Overloads whose types normalize to the same names (an alias such as
    `Key id` for `Text`) clash where they are declared.
  - Each overload is its own kernel function; resolution only picks a
    name, outside the kernel.
  - Then each field of a record without parameters gets a projection,
    `id of (summary)`, defined by a `MATCH`.
- Macros: `MACRO`, `QUOTE`, `UNQUOTE`, with a call whose name holds a keyword
  being a macro use.
- Effects: monads as library types, or effect rows.
- More universe levels, only if two prove too few, and whether `MATCH`
  stays in the core or becomes recursors.
