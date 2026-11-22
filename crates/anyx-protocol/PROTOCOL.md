# The anyx protocol

An app declares its methods in a schema written in anyx. A request is an
anyx document of calls in the methods' shapes; the response is an anyx
document of their results. Both are checked against the schema.

## The schema

```anyx
# What the protocol is for, for whoever reads it first.
PROTOCOL poentaro VERSION 1,

# A file in the account's tree.
TYPE ALIAS File Summary {
    # A slash path, such as `Hymns/hymn.poentaro`.
    path (Text),
    revision (Integer),
    note (Text ? ) DEFAULT note (`null),
},

TYPE ALIAS Shade [| `dark, `light |],

TYPE ALIAS Colour [| `dark, `light, `custom hue (hue: Float), `(base: Shade) dimmed |],

# Which file, among every account's.
NAMED TYPE File Id (value: Text),

TYPE ALIAS File Errors [|
    # The owner moved this file to the trash.
    `trashed,
    `missing,
    `(path: Text) would be both file and folder,
|],

# Saves the score, if the file is still at `revision`.
METHOD write (path: Text) at revision (revision: Integer) content (content: Text)
    RETURNS (new revision: Integer)
    ERRORS [|
        # Someone saved first. Apply your change to `content`, then write at `revision`.
        `conflict at revision (revision: Integer) content (content: Text),
        File Errors,
    |]
    EXAMPLES [
        # Saving over the revision you read.
        write "Hymns/a.poentaro" at revision 3 content "title" RETURNS 4,
        # Someone saved first.
        write "Hymns/a.poentaro" at revision 3 content "title" ERROR `conflict at revision 4 content "hymn",
    ],
```

- **`PROTOCOL name VERSION n`** comes first. `n` is a whole number from 1.
- **`#` lines** before a definition, a field or a variant document it. They
  are part of the schema, so the guide an agent reads includes them. `##`
  starts a comment. A doc is always `#` lines: a quoted `"…"` or raw
  `"""…"""` string where a doc goes is an error.
- **Names** of types are written in Title Case: `File Summary`, `File Id`.
  Other names, of fields, parameters, methods, variants and errors, are
  lowercase words: `new revision`. All-caps words belong to the schema
  language, so a word with no lowercase letter is a keyword: write `Id`,
  never `ID`. Generic shapes can use lowercase connective words, as in
  `Page of (Item: TYPE)`. Keywords side by side join into one name, so write
  `LIST: LIST Text` with the `:`; a
  formatted schema drops a `:` that nothing would join, as in `LIST Text`.
  Braces and union brackets group the type body in a declaration, so the
  formatted spelling is `TYPE ALIAS Name { … }` or `TYPE ALIAS Name [| … |]`.
- **Types:** the built-in types `Text`, `Bytes` (such as a file), `Integer`
  (64-bit), `Float` (a finite 64-bit IEEE 754 float: `f64` in Rust,
  `number` in TypeScript), `Boolean`, `Instant` (a point in time) and
  `Duration` (a length of time), declared in the [prelude](#the-prelude);
  `LIST: T`, `MAP: T` (text keys, each to a `T`), `{ field (T) }`, `[| A, B |]`,
  the name of a record, named type or alias, or a use of a generic type such
  as `Page of (Order)`. A type may hold itself only through a
  `LIST` or a `MAP`, which may be empty.
- **`NAMED TYPE File Id (value: Text)`** makes a type of its own, which no other
  type fits, so that values that hold the same thing but mean different
  things can't be mixed up. Each slot in its name declares a name and a type, and
  a value is written with that shape and a value in each slot: `File Id
  "a"`. A slot may be anywhere among the words, and there may be several or
  none: `` NAMED TYPE (value: [| `red, `blue |]) Color `` is written `` (`red) Color ``,
  `NAMED TYPE Range from (start: Integer) to (end: Integer)` is written `Range from 1 to
  3`, and `NAMED TYPE Unit` is written `Unit`. Since a value shows its type,
  it needs no annotation, and a union of named types tells its members
  apart by name. A use names it by its words, `Range from to`. In JSON a
  value is an object whose one key is the type's words in camel case,
  starting with a capital. It holds the value directly for one slot, an
  object keyed by slot name for several, and an empty array for none:
  `{ "FileId": "a" }`, `{ "RangeFromTo": { "start": 1, "end": 3 } }`, and
  `{ "Unit": [] }`. Rust generates a newtype for one slot and a struct with
  named fields for several. TypeScript generates a branded type. A slot's type may hold the named type only
  through a `LIST` or a `MAP`. A named type can wrap a record in a slot.
  `NAMED TYPE Quiet Hours (schedule: { quiet from (start: Integer) to (end: Integer) })`
  is written `Quiet Hours { quiet from 22 to 7 }`.
- **`TYPE ALIAS Name: T`** names a type without making a new one:
  `TYPE ALIAS Paths: LIST Text`. Each use of `Paths` is `LIST Text`, on the
  wire, in JSON, in the guide and in generated code, so an alias and its
  type mix freely. A generic alias has a type parameter in each other slot
  of its name, as a generic record does, and its type uses them:
  `TYPE ALIAS (Item: TYPE) Pair: LIST Item`, used as `(Text) Pair`. The
  type it stands for comes last and can't lead back to the alias, since an
  alias is only another name. Record aliases and named types can hold
  themselves through a `LIST` or a `MAP`. `PUBLIC` shares an alias with direct importers, as it does a
  type. Use `NAMED TYPE` when values should not mix with others that hold
  the same thing, and `TYPE ALIAS` when a long type only needs a short name.
  An alias of a union of variants alone keeps its name in generated code,
  as an enum, and in the guide and messages: see below.
  A record alias such as `TYPE ALIAS Name: { field (T), … }` keeps its name
  as a generated struct or object type and fits structurally. A value writes
  each field in its declared shape, in any order. See [Records](#records).
  `DEFAULT` after a whole field makes it omittable, and says, as an entry
  written as a value writes it, what it holds when absent:
  `limit (Integer) DEFAULT limit 50`, or `tags (LIST Text) DEFAULT tags []`.
  Without a default the field must be supplied. A default is checked as an
  entry of its field. A record can also appear directly in a type slot:

  ```anyx
  TYPE ALIAS Lookup Result [| { count (Integer) }, `null |],
  METHOD search (filter: { query (Text) }) RETURNS (result: Lookup Result) ERRORS [| |],
  ```

  Anonymous records in generated Rust and TypeScript receive sequential names
  such as `InlineRecord1`. Use a record alias when its generated
  name is part of the API. In requests, a method argument with an inline
  record type is checked against the method\x27s schema without an `AS` annotation.
- **`[| A, B, … |]`** is a value of one of its members, written as that
  member writes it, on the wire and in JSON: a value of
  `` [| Integer, `none |] `` is `3` or `` `none ``. A member is a variant,
  written with a backtick, or a named type, an intrinsic, a refinement
  or another union or an inline record. A variant or a named type may have a
  `#` doc before it. A `LIST` or `MAP` member uses a `TYPE ALIAS`. A value's
  form alone tells its member, so the checker flattens a union and rejects
  two members that share a form in anyx or in JSON:
  `[| Integer, Float |]` shares the number, and `Depth??` repeats `` `null ``.
  `[| |]` has no values. A union fits where another is expected when
  each of its members does, so a `Depth` fits where a `Depth?` is expected.
- **Nullability** is the prelude's `T?`, the alias `` [| Item, `null |] ``:
  `` `null ``, or a value of the item written as it is, `note "hi"`.
  `note: Text?` is required but admits `` `null ``;
  `` note (Text?) DEFAULT note (`null) `` may be omitted. `OPTIONAL` is no longer a
  protocol keyword. The parser generator's `OPTIONAL` combinator is unchanged.
- **`` TYPE ALIAS Name [| `variant, `… |] ``**, a union of variants
  alone: one of its variants. A variant is a *shape*: words, with
  `(name: Type)` slots anywhere among them, as in `custom hue (hue: Float)`
  or `(base: Shade) dimmed`. Variants differ by their words. Such an alias
  keeps its name: generated code has an enum `Name`, and the guide and
  messages say `Name`. Its values are its variants, written the same, and
  it can't lead back to itself. `` CHOICE Name { `variant
  } ``, which it replaced, is an error that says how to write it.
- **A generic record or type alias** has type parameters in its name. Its
  name is a shape, like a variant's: words, with a `(Parameter: TYPE)` slot
  anywhere among them, at the start, between words, or at the end. Its
  fields or variants use each parameter as a type. A use writes the same
  words with a type in parentheses in each slot, in the same places:
  `Page of (Order)`, `(Receipt) or (Decline)`. A value of it is written
  exactly like the record or union with those types in place of the
  parameters, on the wire and in JSON, and checked against them, in
  `DEFAULT`s and `EXAMPLES` too.

  ```anyx
  # A page of a long list, and where the next one starts. (A slot at the end.)
  TYPE ALIAS Page of (Item: TYPE): {
      items (LIST Item),
      next (Text?) DEFAULT next (`null),
  },

  # What came of a payment, or of any attempt. (Slots at the start and the end.)
  TYPE ALIAS (Value: TYPE) or (Problem: TYPE) [|
      `done (value: Value),
      `failed (problem: Problem),
  |],

  # A value and who vouches for it. (A slot between words.)
  TYPE ALIAS (Value: TYPE) signed by (Signer: TYPE): {
      value (value: Value) signed by (signer: Signer),
  },

  METHOD orders RETURNS (orders: Page of (Order)) ERRORS [| |],
  METHOD pay (order: Order Id)
      RETURNS (outcome: (Receipt) or (Decline))
      ERRORS [| |],
  METHOD pay all (orders: LIST Page of (Order Id))
      RETURNS (outcomes: Page of ((Receipt) or (Decline)))
      ERRORS [| |],
  METHOD search (words: Text)
      RETURNS (results: Page of ((Document) signed by (Person)))
      ERRORS [| |],
  TYPE ALIAS Orders: Page of (Order),
  ```

  A slot may also sit between words in the middle of a name,
  `TYPE ALIAS Box of (Item: TYPE) sealed: { item (Item) }`, used as `Box of (Letter) sealed`, and
  a name may have several slots side by side with words around them,
  `TYPE ALIAS Table from (Key: TYPE) to (Value: TYPE): { keys (LIST Key), values (LIST Value) }`.

  A type is known by its words and where its slots are, so `Page` and
  `Page of (Item: TYPE)` are two types, and both may be declared; two
  declarations of the same shape are an error. A `TYPE ALIAS` can declare
  type parameters, and
  `PUBLIC TYPE ALIAS Page of (Item: TYPE): { items (LIST Item) }` shares one. A parameter is a
  Title Case name, as a type's is, marked by the keyword `TYPE`, not a
  type, so that no type of types is needed. It is once in its declaration, used by at least one field or
  variant, and not the name of a type visible there: a type declared in its file,
  or a directly imported `PUBLIC` type. A parameter has no slots, and a field
  whose type holds one can have a `DEFAULT` only if it can be checked
  without knowing the argument, as in `` content (Item?) DEFAULT content (`null) ``. A use fills every slot, in order; a use whose
  words or slots match no declaration is an error that names the types
  with those words. Within its own declaration, a generic type uses itself
  with its own parameters as they are, `LIST Tree of (Item)`, and only
  through a `LIST` or a `MAP`. An imported type uses its plain shape, `Page of (Order)`.

  Make a type generic only when it is used with different arguments, such
  as `Page of (Order)` and `Page of (File)`. If every use passes the same
  types, a plain record says the same thing more simply, and its generated
  name reads better: `TYPE ALIAS Trashed File: { file (File Summary), trashed at (Instant) }`,
  not `TYPE ALIAS (Item: TYPE) in trash since (When: TYPE): { item (Item), when (When) }` used only as
  `(File Summary) in trash since (Instant)`.
- **`METHOD shape RETURNS (name: Type) ERRORS [| errors |]`**: the method's
  shape, what it returns, and the errors it may return. The returned value
  has a name, which says what it is. `ERRORS` is always written, so that a
  reader sees a method's errors were declared up front: a method that
  can't fail says `ERRORS [| |]`. A method with nothing to return, such as `sign out`, says `RETURNS NOTHING`. `READ ONLY`
  or `IDEMPOTENT` may come before `METHOD`, and `UNDONE BY` after `ERRORS`:
  see [Effects](#effects).
- **`EXAMPLES [example, …]`**, last in a method, shows how it is used. Each
  example is a call of the method and its outcome side by side, since it
  shows a request and its response together: the call by its full name, as
  a request writes it, then the result as a response writes it, `RETURNS`
  and a value, `RETURNS NOTHING`, or `ERROR` and one of the method's
  errors. A `#` line before an example says what it shows. The
  schema is checked against its examples, so that they can't go stale:
  each call must be a call of its method, and each outcome one the method
  may have. The guide and generated code show them.
- **Errors** are variants, written in a method's `ERRORS` brackets with
  their `#` docs before them, since an error says by its words what went
  wrong: `` ERRORS [| `conflict at revision (revision: Integer) |] ``. Errors
  that methods share are an alias of a union of variants, named among
  them: `` TYPE ALIAS File Errors [| `trashed, `missing |] `` and
  `` ERRORS [| `conflict at revision (revision: Integer), File Errors |] ``. Such
  an alias may include another: `` TYPE ALIAS Export Errors
  [| `unreadable, File Errors |] ``. Nothing else goes in `ERRORS`: a value
  an error carries goes in a slot of its variant.
- A method's errors are flattened: its own variants and those of each
  alias it names, in order, each once. The generated code has one error
  type for each method, holding them all.
- **Overloads:** methods may share a shape when at least one parameter
  position accepts disjoint types, as in `METHOD scale (factor: Float)`
  and `METHOD scale (times: Integer)`. This rule applies to every form
  produced by omitting optional parts. `UNDONE BY` writes the parameter
  types to name an overload.

  Records are disjoint if either requires a field the other lacks, or if
  a shared field required by either record accepts disjoint types. Unions
  are disjoint if every pair of members is disjoint. Two nullable types
  share `` `null ``. Two lists share `[]`, and two maps share `{}`,
  regardless of their element types. Refinements are compared by their
  base types. Rename one method if its arguments can also fit another.
- A parameter can't be named `kind`, which JSON uses for a variant's name.

### Refinements

`WHERE` narrows a type with a predicate. It is a type expression, the call
`_ WHERE _`, so it works anywhere a type does: a `TYPE ALIAS`, a field, a
parameter, a return, or an argument of a generic type.

```anyx
TYPE ALIAS Revision: Integer . WHERE [1 <= _],
TYPE ALIAS Probability: Float . WHERE [0 <= _ <= 1],
TYPE ALIAS Score Path: Text . WHERE [_ ends with ".poentaro"],
TYPE ALIAS Limit: (Integer) WHERE [10 <= _ < 30],
TYPE ALIAS Settings: { limit (Limit) DEFAULT limit 10 },
METHOD limit (value: Integer . WHERE [10 <= _ < 30])
    RETURNS (value: Integer . WHERE [10 <= _ < 30])
    ERRORS [| |],
```

The dot supplies the base type as the first argument; parentheses around the
base type work too. The predicate stays inside square brackets so that its
`_` is not consumed by dot or colon sugar. `Integer WHERE […]` also has the
right shape because `Integer` is a name, unlike the old keyword `INTEGER`:
`INTEGER WHERE […]` is the different call `INTEGER WHERE _`. Use dot or
parentheses to make the intended grouping explicit, especially with `LIST`
and `MAP`.

Supported predicates are deliberately small and checked when loading the
schema:

- Comparisons `<`, `<=`, `>`, `>=`, `=`, `!=`, including chains with mixed
  bounds, such as `10 <= _ < 30`. Each adjacent pair must hold. The prelude
  declares the supported chain shapes.
- Numeric `_` for `Integer` or `Float`, with numeric constants. Integer
  predicates use whole-number constants and exact checked signed 128-bit
  arithmetic; overflow fails the predicate. Float predicates use finite
  64-bit arithmetic, and overflow fails too.
- Parenthesized linear arithmetic: `(_ + 1) <= 30`, `(_ * 2) > 0`.
  Multiplication needs a constant operand; multiplication of two expressions
  that depend on the value, division, and other operations are rejected.
  Symbol words have no precedence: `_ + 1 <= 30` is one unrecognized call,
  so it needs parentheses around the addition.
- `(_ length)` for `Text`, `Bytes`, `LIST`, and `MAP`. Text length counts
  Unicode scalar values, bytes count bytes, lists count items, and maps
  count entries. Length bounds are whole numbers.
- Text tests `_ starts with "prefix"`, `_ ends with "suffix"`, and
  `_ contains "part"`, matched exactly and case-sensitively.
- Boolean functions `not`, `and`, `or`, and `xor` combine predicates.
  Group nested calls with parentheses, dot, or colon.
- Several predicates separated by commas mean all of them must hold:
  `Text . WHERE [1 <= (_ length), _ ends with ".poentaro"]`.

A predicate must mention `_`. The base type must be concrete enough to check
its operations; an unconstrained type parameter cannot itself be compared
numerically. Refined types can still be passed as generic arguments, or
used in a generic declaration's `LIST` length constraint. Type annotations
compare the predicate's structure, ignoring its whitespace and source spans.
Overloads cannot be distinguished solely by refinements of the same type;
use distinct `NAMED TYPE`s to identify them. How one refinement fits
another is under [Subtyping](#subtyping). Satisfiability, and relations
between several method arguments or results, are outside this feature.

The wire and JSON keep the base type's representation, and a value needs
no annotation beyond its base type's: `write "a.poentaro" at revision 1`.
An annotation names the refinement as written, `(… AS: Integer . WHERE [1
<= _])`, since an alias makes no type of its own. A refinement in a `NAMED
TYPE`'s slot applies to its slot's value: `NAMED TYPE Revision (value: Integer .
WHERE [1 <= _])` is written `Revision 4`.

A failed argument, including a nested field or item, rejects the whole
request as `INVALID REQUEST` before any call runs. The diagnostic identifies
the call and argument, source location, value, type, written predicate, its
meaning in words, and which bound or text test failed. Defaults and method
examples are checked while loading the schema. JSON input uses the same
validation through its existing error result, with field and argument context.
A returned value or declared error payload that breaks its refinement is
an encoding error; generated servers turn it into `INTERNAL ERROR` and stop
before running the next call. A client rejects an invalid returned value.
The generated guide explains both the predicate and its bounds in words.

### Subtyping

A value of one type may be passed where another is expected when every
value of the first is a value of the second. The shell checks this when
one call's result is another call's argument, before anything runs.

- **A refinement fits its base,** so `Depth`, `` TYPE ALIAS Depth: Integer
  WHERE [0 <= _] ``, fits `Integer`. It fits another refinement only when
  their predicates are the same. A base never fits its refinement: an
  `Integer` result doesn't promise `0 <= _`, so `dig (count)` is rejected
  when `count` returns `Integer`. Declare `count` to return `Depth`. A
  literal is checked by its value instead, so `dig 3` works.
- **A member fits its union,** and a union fits another that holds each of
  its members: `` [| `dark, `light |] `` fits `` [| `dark, `light,
  `custom hue (hue: Float) |] ``. An alias is the type it stands for.
- **`LIST`, `MAP`, `?` and a generic record's slots are covariant:** `LIST
  Depth` fits `LIST Integer`, and `Page of (Depth)` fits `Page of
  (Integer)`.
- **A record fits a larger one** when each field it lacks has a `DEFAULT`:
  `{ x (Integer), y (Integer) }` fits `{ x (Integer), y (Integer), label (Text)
  DEFAULT label "" }`. A field the larger record lacks never fits, since
  the receiver would reject it.
- **A named type fits only itself.** `File Id` doesn't fit `Text`, nor
  `Text` it, nor another named type that holds `Text`.

There is no type inference and no intersection of types.

### Method conditions

A method can declare a `REQUIRES` clause before `RETURNS` and an
`ENSURES` clause after it.

```anyx
METHOD write (path: Text) at revision (revision: Integer) before (limit: Integer)
    REQUIRES [1 <= revision < limit]
    RETURNS (new revision: Integer)
    ENSURES [new revision =: revision + 1]
    ERRORS [| |],
```

Conditions share the refinement predicate language. They refer to the
method's named arguments, and `ENSURES` may also refer to the named result.
The supported call shapes and overloads are declared as `FUNCTION` in
[`prelude.protocol.anyx`](prelude.protocol.anyx). Each bracket entry must
produce a `Boolean`. Functions such as `+` and `length` provide operands
to other calls. The codec evaluates these functions locally. Only the
bundled prelude can declare a `FUNCTION`. Use parentheses, dot, or
colon to group nested calls. For example, `new revision =: revision + 1`
and `new revision = (revision + 1)` express the same condition. Each test
in the brackets must hold. Use refinement types for restrictions on
individual values that apply wherever those types are used.

Two prelude functions test a whole list, so only conditions use them.
They let a method take a list of settings, rather than optional words,
and still rule out settings that contradict each other:

```anyx
TYPE ALIAS Setting [| `any case, `smart case, `hidden, `(name: Text) encoding |],

READ ONLY METHOD find (pattern: Text) with (settings: LIST Setting)
    REQUIRES [
        settings . has no repeats,
        settings . has at most one of [`any case, `smart case],
    ]
    RETURNS (paths: LIST Text)
    ERRORS [| |],
```

- `items . has no repeats` holds when no two items are equal.
- `items . has at most one of [a, b]` holds when at most one item is a
  variant of the shapes listed. `items` is a `LIST` of a union of variants, and each
  shape is one of its variants, written by its words with `_` in each slot:
  `[_ encoding]` allows one `encoding` setting, whatever its text.

The decoder checks every call's preconditions before dispatch. A failure
returns one `INVALID REQUEST` for the whole batch, with the call's location,
the values involved and the failed condition. The producer checks
postconditions on successful outcomes. A violation is logged and that
outcome becomes `INTERNAL ERROR`. Generated servers stop the batch there.
The JSON bridge uses the existing outcome format with `"kind": "internalError"`
for that result. The guide includes the clauses, and example calls and
successful outcomes are checked when the schema loads.

### Writing shapes

A shape is read as a sentence: in a call, an error and the guide. Write it
so the sentence says what a `#` doc would, and keep `#` for what no shape
can say.

- **Put each slot where its word goes**, at the start, middle or end:
  `(path: Text) would be both file and folder`, `(email: Text) is not a
  test account`, `move (path: Text) to (new path: Text)`. On the wire it
  reads `"Hymns/a.poentaro" would be both file and folder`.
- **Words after a slot give its unit**, and the slot's name says what the
  value is: `locked for (duration: Integer) seconds` reads `locked for 30
  seconds`. Don't repeat the unit as the name, as in `(seconds: Integer)
  seconds`.
- **Don't write a `#` doc that restates the shape**, like `# The path would
  be both a file and a folder.` above `(path: Text) would be both file and
  folder`. Keep a doc for a rule the shape can't carry, such as
  `# Nothing moved.`
- **A shape whose slot is a sentence keeps it last**: `weak password
  (message: Text)`, since no words could follow a message well.
- Generated code names a variant by its words alone, so
  `locked for (duration: Integer) seconds` is `LockedForSeconds { duration }`.
  Two variants can't differ only by where their slots sit.

### Optional parts

A part of a method's or a variant's shape in square brackets may be left
out of a call. The part is words and slots, then `DEFAULT` and the part as
a call writes it, which a call without the part holds:

```anyx
READ ONLY METHOD find (pattern: Text)
    [in (paths: LIST Text) DEFAULT in ["."]]
    [with (search: Search) DEFAULT with {}]
    RETURNS (paths: LIST Text)
    ERRORS [| |],
```

- **The full form is the method.** `find _ in _ with _` is the one method
  a server implements, and its shape is the full form. `find "x"` is sugar
  for `find "x" in ["."] with {}`: the request decoder fills in each
  left-out part's default, so a server and the codec only see full calls.
- **A call may leave out any of the parts**, and the rest keep their
  order: `find "x"`, `find "x" in ["src"]`, `` find "x" with { `hidden } `` and
  `` find "x" in ["src"] with { `hidden } ``. `find "x" with {} in ["src"]` is
  an error.
- **A part has a word and a slot.** Slots side by side can't be told apart,
  so `[(factor: Integer) DEFAULT 1]` is an error; and a part with no slot
  carries nothing a call doesn't, so `[descending DEFAULT descending]` is
  an error too. Use an unnamed field with a default for a flag in a record.
- **No two ways to write a call may clash.** Each way to write a method,
  with parts left out, must differ from every other method's in its words
  or a parameter's type, and each way to write a variant from every other
  variant's in its words. `find (pattern: Text)` next to the `find` above
  is an error, since both are written `find "x"`.
- **A default is checked as the part.** `DEFAULT in "."` is an error,
  since `paths` is a `LIST Text`, and so is `DEFAULT at ["."]`, which isn't
  a call of `in _`.
- **JSON carries the full form.** A method's name in JSON is its full
  form's, and its arguments are all of them.
- **Generated clients get a function per form.** A server implements the
  full form; a client has one function for each way to write the call,
  and each shorter one calls the full form with the defaults filled in:
  `find(pattern)` calls `findInWith(pattern, ["."], {})`.

A `Duration` is written with optional parts too: its units are those of
the prelude's `INTRINSIC FUNCTION`, so `1 hours 30 minutes` leaves out the
days, seconds and milliseconds.

### Records

A record field is a shape with words and slots. An Anyx value fills the
slots in shape order. For example, one field can keep the start and end
of quiet hours together:

```anyx
TYPE ALIAS Quiet Hours { quiet from (start: Integer) to (end: Integer) },
```

Its value is `{ quiet from 22 to 7 }`.

| Declared | Written | Named |
| --- | --- | --- |
| `path (Text)` | `path "a"` | `path` |
| `` (Case) case DEFAULT (`smart) case `` | `` (`any) case `` | `case` |
| `` at least (Size?) bytes DEFAULT at least (`null) bytes `` | `at least 14 bytes` | `at least bytes` |
| `between (start: Integer) and (end: Integer)` | `between 1 and 3` | `between` |
| `` `(hidden: [| `hidden, `null |]) DEFAULT `null `` | `` `hidden `` | `hidden` |
| `` `(order: Order) DEFAULT `newest first `` | `` `by name `` | `order` |

- **Prefer parentheses around field slots.** Write `path (Text)` or
  `(Case) case`. The shorthand `name: Type` is discouraged because it
  suggests a key–value pair. Put words around slots where they help the
  value read as a phrase.
- **A multi-slot field names each slot.** Its first word names the field.
  The field's value is a named struct in Rust and an object in TypeScript
  and JSON. For example, `between 1 and 3` contributes
  `"between": { "start": 1, "end": 3 }` to its JSON record.
- **A `DEFAULT` takes the whole field**, and is the entry the field holds
  when left out, written as a value writes it: `age (Integer) DEFAULT age 0`,
  `between (start: Integer) and (end: Integer) DEFAULT between 1 and 9`,
  `` `(order: Order) DEFAULT `newest first ``. With a `:`, the field goes in
  parentheses, `(age: Integer) DEFAULT age 0`, since `age: Integer DEFAULT
  age 0` is `age (Integer DEFAULT age 0)`, a `DEFAULT` in the slot.
- **A value is written as its type writes it**, a nullable one as its item
  or `` `null ``: `at least 14 bytes`, `` at least (`null) bytes `` for a `Size?`.
- **No `Boolean?`.** Three states named `` `true ``, `` `false `` and `` `null ``
  leave the reader to guess what `` `null `` means. Use a nullable variant with a default when left out means
  off, or a union of variants that names each state:
  `` TYPE ALIAS Color [| `always, `never, `auto |] ``, not
  `color (Boolean?)`.
- **A value before the words keeps its parentheses**, `` (`any) case ``:
  without them, `` `any case `` is the one variant `` `any case ``, not `_ case`.
- **Each field once.** A record says each thing once, so where a list of
  settings needs `REQUIRES` to rule out `[glob, fixed text]`, a record's
  `(Syntax) pattern` is one choice.

**Fields written as their variant.** `` `(order: Order) `` is a backtick
and a slot with no words, so a parameter names the field. A value writes
the field's variant alone, `` `by name ``: an entry that starts with a
backtick can't be a field's shape, so it is the value of the field whose
type has that variant. An unnamed field takes a union whose members are
all variants.
Such fields may share a slotless variant only when it is the `DEFAULT` of
every field that shares it. For example, two nullable fields can share
`` `null `` when both default to it. Leave the fields out to use a shared
default. Writing it explicitly is ambiguous.

Readability decides between them, since a record is read far more than it is
written:

1. **On or off → a nullable variant with a default.** Write `` `hidden ``
   or `` `follow links `` to turn on that option.
2. **Options that combine → separate fields, not a choice.** `no ignore`
   and `no ignore vcs`; `changed within` and `changed since`. A choice is
   for options that truly exclude each other.
3. **A value with a word that names it → a shape.** `max depth 3`,
   `at least 14 bytes`, `retry 3 times`. This is the default, and the only
   form for text, numbers and lists, whose form never says what they mean:
   `"src"` and `3` alone would leave the reader guessing, which is why a
   field without words holds only variants.
4. **A choice only one field uses → its word in every variant, and the
   variant alone.** `` TYPE ALIAS Case [| `smart case, `any case, `exact case |] ``
   with `` `(case: Case) DEFAULT `smart case `` is written `` `any case ``.
   Every variant carries the word, so that none reads bare, as `` `exact ``
   would.
5. **A choice several fields or methods share → its shared word in the
   shape.** `(Case) case` over `` TYPE ALIAS Case [| `smart, `any, `exact |] ``,
   written `` (`any) case ``, keeps the word out of the type, where it would follow the
   type everywhere it is used.
6. **A choice whose variants are phrases that say what they are, share no
   word, and would stutter with one → the variant alone.**
   `` `newest first ``, `` `by name ``, `` `no auth ``, where a word would
   only repeat (`` (`no auth) auth ``) or add nothing (`` (`by name) order ``).
   Read `` `by name `` alone: if it says nothing to someone reading the call
   cold, give it a word.

When in doubt, use a shape: a word costs one word, while an unclear variant
alone makes every reader work it out.

### Naming methods

Name a method after what the caller wants done, not after the record it
changes. Most real operations change several things at once and keep a rule
between them, such as a balance that never goes below zero. A client that
does them with a create, a read, an update and a delete for each record has
to keep that rule itself, and a request's calls run in order, not as one
transaction, so a failure halfway leaves the change half made. Agents guess
at such steps, which makes it worse.

```anyx
## Not this: the client works out what a transfer changes, and in what order.
METHOD update account (id: Text) fields (fields: account fields)
    RETURNS (account: account)
    ERRORS [| |],

## This: the server does the whole transfer, or none of it.
METHOD transfer (amount: Integer) cents from (source: Text) to (target: Text)
    RETURNS (transfer: Text)
    ERRORS [| `short by (shortfall: Integer) cents |],
METHOD close (account: Text) RETURNS NOTHING
    ERRORS [| `still holds (balance: Integer) cents |],
```

- **The shape names the intent.** `transfer (amount) cents from (source) to
  (target)` reads as what the caller means; `update account (id) fields
  (fields)` says only what it touches.
- **Reads may be uniform.** Listing and filtering look alike for every
  record, so `list accounts (filter: account filter)` is fine. Keep writes
  as intents.

Schemas are formatted with `anyx_protocol::format_schema`, which puts a long
`METHOD`'s clauses on lines of their own.

### Effects

Words around a method say what a call of it does to the world, so that a
client can decide on its own whether to retry a call whose response was
lost, and whether to ask a person before sending it.

```anyx
READ ONLY METHOD read (path: Text) RETURNS (document: Document)
    ERRORS [| `missing |],

IDEMPOTENT METHOD archive (path: Text) RETURNS NOTHING
    ERRORS [| `missing |]
    UNDONE BY unarchive (path: Text),

IDEMPOTENT METHOD unarchive (path: Text) RETURNS NOTHING ERRORS [| `missing |],

METHOD move (path: Text) to (new path: Text) RETURNS NOTHING
    ERRORS [| `missing, `taken |]
    UNDONE BY move (new path: Text) to (path: Text),

## No markers: may change anything, and is never retried on its own.
METHOD append (text: Text) to (path: Text) RETURNS NOTHING ERRORS [| `missing |],

## Can't claim `UNDONE BY`: the recipient has read the document.
METHOD send (path: Text) to (email: Text) RETURNS NOTHING ERRORS [| `missing |],
```

- **`READ ONLY`** before `METHOD`: the caller asks for no change, as HTTP's
  *safe* methods do. The server may still log or count calls. Safe to call
  freely.
- **`IDEMPOTENT`** before `METHOD`: calling it again with the same arguments
  leaves the same state as calling it once, so it is safe to retry after a
  lost response. This is about the effect, not the response (RFC 9110): a
  second `archive` may answer with an error that says it is already done,
  and is still idempotent. Prefer making a repeated call succeed where you
  can. `READ ONLY` already means idempotent, so `READ ONLY IDEMPOTENT` is an
  error.
- **No marker is the worst case**: the call may change anything, may do it
  again when sent again, and can't be undone. A client doesn't retry it
  blindly, and an agent asks before sending it when unsure. Every marker is
  a claim of safety, so leaving one out is never unsafe, as with MCP's tool
  annotations and Protobuf's `IDEMPOTENCY_UNKNOWN`.
- **`UNDONE BY call`**, after `RETURNS` and `ERRORS`, names the call that
  compensates the method's effect, as a step of a saga is compensated: a
  call of another method of the protocol. It is written like a shape whose
  slots are the method's own parameters, each `(name: Type)` with that
  parameter's name and type: `move (path) to (new path)` is undone by
  `move (new path: Text) to (path: Text)`. The types say which method it
  is, among methods with the same words, so the call must match exactly one
  method: the same words and the same type in each slot. Only parameters fill
  the slots, not the returned value or a literal value.
  - It claims the state afterwards is equivalent to before, not exactly the
    same: an unarchived document may come back with a new revision.
  - It also claims the call **disclosed nothing to anyone outside the
    account** that undoing wouldn't take back. Sending an email, publishing,
    or sharing with another user can't be undone, since someone may have
    seen it, so such a method has no `UNDONE BY`. Only what a call newly
    opens up counts: writing to a document that is already shared doesn't.
  - A `READ ONLY` method has nothing to undo, so it takes no `UNDONE BY`.
    The undoing method may carry any markers of its own, including its own
    `UNDONE BY`.
- Markers are the server's promise, and describe the most a call can do.
  `SCHEMA` is `READ ONLY`.

Generated clients expose each method's markers as data, so that a tool can
build a policy on them, such as running read only calls freely and asking
before anything that can't be undone.

### Files and USE

A protocol is one flat namespace, however many files it uses. An entry file
starts with `PROTOCOL`; its filename is arbitrary. It can contain declarations
alongside explicit imports:

```anyx
## notes/protocol.protocol.anyx
PROTOCOL notes VERSION 1,
USE "./files.protocol.anyx",
USE "./files/export.protocol.anyx",

## notes/common.protocol.anyx
PUBLIC TYPE ALIAS Diagnostic: { line (Integer), message (Text) },
PUBLIC TYPE ALIAS Common Errors [| `missing |],

## notes/files.protocol.anyx
USE "./common.protocol.anyx",
PUBLIC TYPE ALIAS File Errors [| `trashed, Common Errors |],
METHOD read (path: Text) RETURNS (text: Text) ERRORS [| File Errors |],

## notes/files/export.protocol.anyx
USE "../common.protocol.anyx",
USE "../files.protocol.anyx",
METHOD pdf (path: Text) RETURNS (pdf: Bytes)
    ERRORS [| `unreadable (diagnostics: LIST Diagnostic), File Errors |],
```

- `USE "relative path"` is the only import form. It includes that file in
  the protocol and makes its `PUBLIC` types visible in the
  importing file, by their plain names. Private declarations are visible only
  in their own file. All methods in included files join the protocol.
- There is no re-export: importing `files.protocol.anyx` does not make its imports
  visible. Each file must directly import the declarations it uses.
- Every type name must be unique across the entire protocol,
  including private declarations. Duplicate diagnostics name both files.
  Method shapes must be unique except for overloads distinguished by parameter
  types. Files and folders do not affect names.
- Paths are relative to the importing file. `..` is allowed; absolute paths
  and named package paths are rejected. Stray files are never included.
- A file is loaded once, even if several files import it. Circular imports,
  including self-imports, are rejected at the closing `USE`, with the complete
  cycle. Mutually recursive types must be declared in the same file.
- A headerless file is a library and can be shared by several protocols.
  Importing a file with a `PROTOCOL` header is rejected: put shared declarations
  in a headerless library. A library may contain methods; split types and
  methods into separate files if callers need only the types.
- Checking can start from any file, including a library. Code generation needs
  an entry file with a `PROTOCOL` header; a library has no endpoint. Folder
  input is not supported. The generator requires `.protocol.anyx` for the entry
  and its imports; the basename `protocol` is only a naming convention.

Generated Rust uses a single module, and TypeScript a single file, with plain
names. Shared types are copied into each protocol's generated code, so outputs
stand alone: `notes::Session` and `billing::Session` are distinct Rust types.

To migrate a folder schema, add explicit `USE` lines, remove path prefixes
from calls and type references, and rename duplicate declarations.
Checker diagnostics identify missing imports, private names and collisions.

### The prelude

The built-in types are declared in the prelude,
[`prelude.protocol.anyx`](prelude.protocol.anyx), written in the schema language with a `#`
doc on each. It is read and checked before every schema, and its names are
visible in every file, so a schema can't declare a type of the same name:
`TYPE ALIAS Integer: { … }` is an error.

```anyx
# A whole number of 64 bits, written without a point: `3`, `-1`.
INTRINSIC TYPE Integer,

INTRINSIC TYPE Duration,

INTRINSIC FUNCTION
    [(days: Integer) days DEFAULT 0 days]
    [(hours: Integer) hours DEFAULT 0 hours]
    [(minutes: Integer) minutes DEFAULT 0 minutes]
    [(seconds: Integer) seconds DEFAULT 0 seconds]
    [(milliseconds: Integer) milliseconds DEFAULT 0 milliseconds]
    RETURNS (duration: Duration),

TYPE ALIAS Boolean [| `false, `true |],

TYPE ALIAS (Item: TYPE) ? : [| Item, `null |],
```

- **`INTRINSIC`** marks a declaration whose meaning the implementation
  supplies: the prelude gives its name and its doc, and the Rust code
  gives how its values are read, written and generated. Only the prelude
  may use it; `INTRINSIC` in a schema is an error. An `INTRINSIC TYPE` has
  no type after it.
- **An `INTRINSIC FUNCTION`** may be written in a value: a value of the
  type it returns is a call of it, and the implementation computes the
  value from the arguments of its full form. `Duration`'s is the one, so
  `1 hours 30 minutes` is the call `_ hours _ minutes`, with the days,
  seconds and milliseconds left out. Two durations are equal when their
  milliseconds are, so `90 seconds` equals `1 minutes 30 seconds`.
- The prelude also contains ordinary declarations, checked as user types are.
  `Boolean` and postfix `?` are ordinary aliases: both
  are declared here, without intrinsic Rust type or value cases.
- The guide's list of built-in types is the prelude's docs, and a
  `Duration`'s units are the optional parts of the prelude's
  `INTRINSIC FUNCTION`.
- The built-in types were once keywords, `INTEGER`, `TEXT` and so on. An
  old name is an error that names the new one: "found `INTEGER`; expected
  `Integer`". `LIST`, `MAP` and `TYPE` stay keywords.
- Generated code doesn't change: an `Integer` is still `i64` in Rust and
  `number` in TypeScript, and values are written as before. A method's
  typed name, which names an overload in JSON, writes the types as the
  schema does, so it is now `scale (Float)` where it was `scale (FLOAT)`.

## Requests

A request is calls separated by commas, each in a method's shape with a value
in each slot:

```anyx
list files,
write "Hymns/a.poentaro" at revision 3 content """
    title "Ode"
    """,
```

Calls use plain method shapes regardless of the declaring file:
`pdf "a.note", read "a.note"`. File paths never appear in requests.

Every call runs, in order.

`SCHEMA` is a built-in call every server answers, with no code of its own:
it returns the guide (below) as `Text`, without how to send a request,
since the caller just sent one. A client that has only a server's address
learns everything else from it:

```anyx
SCHEMA,
```

`SCHEMA FILES` is the other built-in call. It returns the schema itself, as
a `LIST Schema File`: the entry file first, then each file it imports, each
with its `path` (as the `USE` lines reach it) and its `source`. The prelude
declares the `Schema File` record alias. A client that
checks calls itself, such as the anyx shell, loads the schema from these
files. Both built-ins are `READ ONLY`, and neither is in a scope.

## Values

[`request.grammar.anyx`](request.grammar.anyx) is the fixed grammar for request syntax,
including values, annotations and type names. The checker gives that syntax
its meaning against the schema; duration order and overflow, instant text,
and base64 are checked in Rust.

| Type | Written |
| --- | --- |
| `Text` | `"text"` with `\"`, `\\`, `\n`, `\r`, `\t` and `\u{…}` escapes; or a raw string of three or more quotes, whose closing quotes' indentation is removed from every line |
| `Bytes` | its standard base64 (RFC 4648, padded) as a string: `"TVRoZA=="` |
| `Integer` | a whole number without a point: `3`, `-1` |
| `Float` | a number with a point or an exponent: `1.5`, `3.0`, `1e3`; never `3`, which is an `Integer` |
| `Boolean` | `` `true ``, `` `false `` |
| `Instant` | RFC 3339 text in UTC, to the millisecond: `"2026-11-14T06:00:08.000Z"`; read without the fraction too, `"2026-11-14T06:00:08Z"` |
| `Duration` | whole numbers each followed by its unit, the largest unit first: `90 seconds`, `1 hours 30 minutes` |
| `LIST: T` | `[a, b]` |
| `MAP: T` | `{ "hymn" 3, "ode" 1 }`, each key a string, once, in any order; `"key": value` when the value is a word. Written in the order of the keys' code points |
| a record | `` { path "a", revision 3, `hidden, (`any) case, `by name } ``, each field in its shape, in any order; `name: value` when the value is a word |
| a union of variants | a backtick and the variant's shape filled in: `` `dark ``, `` `custom hue 0.5 `` |
| a `NAMED TYPE` | its name's shape filled in: `File Id "a"`, `Range from 1 to 3`, `` (`red) Color ``, `Unit` |
| a use of a generic type | as the record or union, with its arguments in place of the parameters: `{ items [{ number 7, total 250 }] }` for `Page of (Order)` |

A number's type is how it is written, as in Rust: `3` and `-1` are
`Integer`s, and `3.0`, `1.5` and `1e3` are `Float`s. So every number states
its own type, and an `Integer` where a `Float` goes is an error ("found the
Integer `3`; expected a Float, such as `3.0`"), in a request and in a
schema's `DEFAULT`s and `EXAMPLES` alike. `Float` was called `NUMBER`
before; only the name changed. A `Float` is always 64 bits, since the wire
carries decimal text and a narrower type would only lose precision; a server
narrows the value itself. There is no exact decimal type: write money as an
`Integer` of cents.

A value written as words, a variant or a `Duration`, goes in parentheses in
a slot, where it would otherwise join the words around it:
`` set (`custom hue 0.5) strict (`true) ``, `wait (5 minutes)`. In a record, such a
value follows a `:`: `{ every: 1 hours 30 minutes }`, `{ "tea": 5 minutes }`.

A `Duration` is written as a call of the `INTRINSIC FUNCTION` of the
[prelude](#the-prelude) that returns it, whose every unit is an [optional
part](#optional-parts): `days`, `hours`, `minutes`, `seconds` and
`milliseconds`. A unit is in the plural for every count, `1 hours`, so a
duration is read one way; `1 hour` is an error that says to write
`1 hours`. A unit left out counts 0. Each unit comes at most once, after
the larger ones, since the parts of a call keep their order; a count may be
more than the next unit holds, as in `90 seconds`. A duration is written with every count below the
next unit, and as `0 seconds` when it is empty. Time finer than a millisecond
is dropped, rounding down, for both types.

## Annotations and overloads

`value AS: type` states the type of an argument whose value doesn't show
it: `tag ([] AS: LIST Text)`, `upload ("TVRoZA==" AS Bytes)`. It is the call
`_ AS _`, so in a slot it goes in parentheses. `AS: x` is `AS (x)` by the
`:` sugar, the same call as `AS x` wherever nothing would join; the `:`
matters only before an all-caps type, where `AS LIST` would join into one
keyword. The type is written by its plain name in the protocol's flat namespace:
a built-in type, a named type such as `File Id`, or a generic type. It must be the parameter's type
exactly. An annotation is a checked claim, not a conversion: the value must
fit the type as written, and `[1] AS: LIST Text` is an error.

A value has a type of its own when how it is written shows it, whatever the
methods:

| Written | Its own type |
| --- | --- |
| a number | `Integer` without a point or an exponent, else `Float` |
| a string | `Text` |
| `` `true ``, `` `false `` | the unique matching union of variants, normally `Boolean`; annotate if ambiguous |
| whole numbers each with a unit | `Duration` |
| a list with items | `LIST` of its items' type, when they all have the same one |
| braces of string keys | `MAP` of its values' type, the same way |
| braces of fields | the record of the schema, not generic, that has each field written and is missing none it requires, when only one does |
| a variant | the union of variants of the schema, not generic, with a variant of its shape, when only one has one |
| a named type's constructor, `File Id "abc"` | that named type |

Anything else has no type of its own: `[]`, `{}`, a value of a generic
type, and a record or variant that several types fit. A `Bytes` and an
`Instant` are written like a string, so their own type is never theirs.

Use an annotation when an argument needs to state a type its spelling
leaves open:

```anyx
read (File Id "abc"),
upload ("TVRoZA==" AS Bytes),
tag ([] AS: LIST Text),
```

Without it, the request is `INVALID REQUEST` and none of its calls run:
"in `content` of `upload _`: `"TVRoZA=="` has the type Text of its own, not `Bytes`;
annotate it, as in `upload ("TVRoZA==" AS: Bytes)`". Only a whole argument is annotated; a value inside one is read by the
type around it, so `AS` inside a value is an error. The schema's
`EXAMPLES` are calls, so they follow these rules too; a `DEFAULT` is read as
an entry of its field.

A call of a shape with overloads selects the one its arguments fit.
A record, variant, or collection can be read against the overload's parameter type
when its type cannot be inferred from the schema alone. An annotation
can still state the type of a value whose spelling leaves it open, such
as `upload ("TVRoZA==" AS Bytes)`. When no overload fits, the invalid
request diagnostic lists the overloads by their typed shapes.

`encode_request`, and so the generated clients, annotate each argument
whose value as written doesn't show its parameter's type, so their callers
never write an annotation.

## Responses

One result per call, in the order of the calls, separated by commas. A
result doesn't repeat its call; a reader matches results to calls by
position. Each result says what came of its call:

- `RETURNS` and the value the method returns, or `RETURNS NOTHING` for a
  method that returns nothing;
- `ERROR` and one of the method's errors, filled in. A method that returns
  an error has changed nothing;
- `INTERNAL ERROR`: the server failed, and logged why. The calls after it
  don't run;
- `NOT RUN`: an earlier call failed with `INTERNAL ERROR`.

For the request `files/read "a.poentaro", files/write "a.poentaro" at
revision 3 content "…", files/export/midi "b.poentaro"`:

```anyx
RETURNS { path "a.poentaro", revision 3, content "…" },
ERROR `conflict at revision 4 content "…",
ERROR `missing,
```

A reader checks that there are as many results as calls, and reads each
as an outcome of the method of the call in its place. Since a result
doesn't repeat its call, no argument, such as a password, comes back in
the response, where it could end up in logs, caches and transcripts.

A request that can't be read gets one result in place of all of them, and
none of its calls run:

```anyx
INVALID REQUEST { line 1, column 6, message "found the call `reed _`; expected one of the methods …" }
```

Responses are laid out by `anyx::format`.

## JSON

With the `json` feature, values convert to and from JSON for a TypeScript
client calling the codec through wasm. Names are camelCase. A record is an
object; a variant or an error is an object whose `kind` is its words in
camelCase, next to its parameters:
`{ "kind": "conflictAtRevisionContent", "revision": 4, "content": "…" }`.
A named type's value is an object whose one key is the type's words in
camel case, starting with a capital, holding its one slot's value or an
object keyed by slot name: `{ "FileId": "a" }`,
`{ "RangeFromTo": { "start": 1, "end": 3 } }`, `{ "Unit": [] }`. An `Instant` is its text, as on the wire, and a `Duration` its whole
milliseconds: `5400000` for `1 hours 30 minutes`.
A field with a `DEFAULT` that is left out takes its default. An explicitly
supplied value is always checked against the field's type.
`Boolean` uses an ordinary variant object, `{ "kind": "true" }`. A union's
value is written as its member writes it, so a `Text?` is `"hello"` or
`{ "kind": "null" }`. Generated Rust uses a `Boolean` enum and `Option<T>`
for `T?`, and supports no other union yet; TypeScript uses a union of the
members' types, `string | { kind: "null" }`.
A `MAP` is an object whose keys are kept as written, not camelCased:
`{ "old ode": 1 }`.
A result is `{ "kind": "value", "value": … }` (with no `value` for a method
that returns nothing), `{ "kind": "error",
"error": … }`, `{ "kind": "notRun" }`, `{ "kind": "internalError" }` or
`{ "kind": "invalidRequest", "line": …, "column": …, "message": … }`.

JSON names a method by its full name, `files/read _`. JSON can't tell
`3` from `3.0`, nor a `Bytes` from a `Text`, so an overload is named by
its full name with its parameters' types in its slots, as written in its
namespace: `scale (Float)`, `tag (LIST File Id)`. Any method may be named
that way; an overloaded one must be, and its full name alone is an error
listing the typed names. `decode_request_json` writes an overload's typed
name, and the generated TypeScript calls and keys `EFFECTS` by it.

## The guide

`anyx_protocol::guide(schema, how_to_send)` writes the markdown an agent
reads before its first request, and what `SCHEMA` answers (with
`how_to_send` empty): these rules, briefly, then the schema with
its `#` lines, a section per source file. It lists the overloads of each
shape together, by their typed shapes, and annotates its made-up sample
calls where a request must. Its sample call is the first
example of the widest method with `EXAMPLES`, or, in a schema without
any, one it makes up with a sample value in each slot.
