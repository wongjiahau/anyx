# The anyx protocol

An app declares its methods in a schema written in anyx. A request is an
anyx document of calls in the methods' shapes; the response is an anyx
document of their results. Both are checked against the schema.

## The schema

```anyx
# What the protocol is for, for whoever reads it first.
PROTOCOL poentaro VERSION 1,

# A file in the account's tree.
RECORD File Summary {
    # A slash path, such as `Hymns/hymn.poentaro`.
    path: Text,
    revision: Integer,
    note: Text ? DEFAULT null,
},

CHOICE Shade { dark, light },

CHOICE Colour { dark, light, custom hue (hue: Float), (base: Shade) dimmed },

# Which file, among every account's.
TYPE File Id: Text,

ERRORS file errors [
    # The owner moved this file to the trash.
    trashed,
    missing,
    (path: Text) would be both file and folder,
],

# Saves the score, if the file is still at `revision`.
METHOD write (path: Text) at revision (revision: Integer) content (content: Text)
    RETURNS (new revision: Integer)
    ERRORS [
        # Someone saved first. Apply your change to `content`, then write at `revision`.
        conflict at revision (revision: Integer) content (content: Text),
    ] + file errors
    EXAMPLES [
        # Saving over the revision you read.
        write "Hymns/a.poentaro" at revision 3 content "title" RETURNS 4,
        # Someone saved first.
        write "Hymns/a.poentaro" at revision 3 content "title" ERROR conflict at revision 4 content "hymn",
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
  never `ID`. Title Case is a convention; the checker takes a type name of
  any case. Keywords side by side join into one name, so write
  `LIST: LIST Text` with the `:`; a
  formatted schema drops a `:` that nothing would join, as in `LIST Text`.
- **Types:** the built-in types `Text`, `Bytes` (such as a file), `Integer`
  (64-bit), `Float` (a finite 64-bit IEEE 754 float: `f64` in Rust,
  `number` in TypeScript), `Boolean`, `Instant` (a point in time) and
  `Duration` (a length of time), declared in the [prelude](#the-prelude);
  `LIST: T`, `MAP: T` (text keys, each to a `T`), the name of a
  `RECORD`, `CHOICE` or `TYPE`, and a use of a generic type,
  `Page of (Order)`. A type may hold itself only through a
  `LIST` or a `MAP`, which may be empty.
- **`TYPE Name: T`** names a type, `TYPE File Id: Text`, so that values that
  share a representation but mean different things can't be mixed up. It is
  written exactly like `T`, on the wire and in JSON, and generated code
  keeps it distinct from `T` and from other named types over `T`. `T` is any
  type, `TYPE Tags: LIST Text`, but can't lead back to the named type.
- **`RECORD Name { field: T, … }`**: fields by name. `DEFAULT value`
  after a field's type makes it omittable and supplies its value when absent:
  `limit: Integer DEFAULT 50`, or `tags: LIST Text DEFAULT []`.
  Without a default the field must be supplied. Defaults are checked against
  the field's type; parameters cannot have defaults.
- **Nullability** is the prelude's generic choice `T?`: `null`, or
  `value (item)`. `note: Text?` is required but admits `null`;
  `note: Text? DEFAULT null` may be omitted. `OPTIONAL` is no longer a
  protocol keyword. The parser generator's `OPTIONAL` combinator is unchanged.
- **`CHOICE Name { variant, … }`**: one of its variants. A variant is a
  *shape*: words, with `(name: Type)` slots anywhere among them, as in
  `custom hue (hue: Float)` or `(base: Shade) dimmed`. Variants differ by
  their words.
- **A generic `RECORD` or `CHOICE`** has type parameters in its name. Its
  name is a shape, like a variant's: words, with a `(parameter: Type)` slot
  anywhere among them, at the start, between words, or at the end. Its
  fields or variants use each parameter as a type. A use writes the same
  words with a type in parentheses in each slot, in the same places:
  `Page of (Order)`, `(Receipt) or (Decline)`. A value of it is written
  exactly like the record or choice with those types in place of the
  parameters, on the wire and in JSON, and checked against them, in
  `DEFAULT`s and `EXAMPLES` too.

  ```anyx
  # A page of a long list, and where the next one starts. (A slot at the end.)
  RECORD Page of (item: Type) {
      items: LIST: item,
      next: Text? DEFAULT null,
  },

  # What came of a payment, or of any attempt. (Slots at the start and the end.)
  CHOICE (value: Type) or (problem: Type) {
      done (value: value),
      failed (problem: problem),
  },

  # A value and who vouches for it. (A slot between words.)
  RECORD (value: Type) signed by (signer: Type) {
      value: value,
      signer: signer,
  },

  METHOD orders RETURNS (orders: Page of (Order)),
  METHOD pay (order: Order Id)
      RETURNS (outcome: (Receipt) or (Decline)),
  METHOD pay all (orders: LIST Page of (Order Id))
      RETURNS (outcomes: Page of ((Receipt) or (Decline))),
  METHOD search (words: Text)
      RETURNS (results: Page of ((Document) signed by (Person))),
  TYPE Orders: Page of (Order),
  ```

  A slot may also sit between words in the middle of a name,
  `RECORD Box of (item: Type) sealed`, used as `Box of (Letter) sealed`, and
  a name may have several slots side by side with words around them,
  `RECORD Table from (key: Type) to (value: Type)`.

  A type is known by its words and where its slots are, so `Page` and
  `Page of (item: Type)` are two types, and both may be declared; two
  declarations of the same shape are an error. Only a `RECORD` or a
  `CHOICE` is generic, `TYPE Name: T` is not, and
  `PUBLIC RECORD Page of (item: Type)` shares one. A parameter is a
  lowercase name, once in its declaration, used by at least one field or
  variant, and not the name of a type visible there: a type declared in its file,
  or a directly imported `PUBLIC` type. A parameter has no slots, and a field
  whose type holds one can have a `DEFAULT` only if it can be checked
  without knowing the argument, as in `content: item? DEFAULT null`. A use fills every slot, in order; a use whose
  words or slots match no declaration is an error that names the types
  with those words. Within its own declaration, a generic type uses itself
  with its own parameters as they are, `LIST Tree of (item)`, and only
  through a `LIST` or a `MAP`. An imported type uses its plain shape, `Page of (Order)`.

  Make a type generic only when it is used with different arguments, such
  as `Page of (Order)` and `Page of (File)`. If every use passes the same
  types, a plain record says the same thing more simply, and its generated
  name reads better: `RECORD Trashed File { file: File Summary, trashed at: Instant }`,
  not `RECORD (item: Type) in trash since (when: Type)` used only as
  `(File Summary) in trash since (Instant)`.
- **`METHOD shape RETURNS (name: Type) ERRORS errors`**: the method's
  shape, what it returns, and the errors it may return. The returned value
  has a name, which says what it is. `ERRORS` may be left out. A method with
  nothing to return, such as `sign out`, says `RETURNS NOTHING`. `READ ONLY`
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
- **Errors** are written where they are used, in full: a shape like a
  variant's, with its `#` doc before it. A method's `ERRORS` has its own
  errors in brackets, then the groups it shares joined with `+`:
  `[conflict at revision (revision: Integer)] + file errors`, or a group
  alone, `file errors`.
- **`ERRORS name [errors]`** defines a group of errors that methods share.
  It may join other groups with `+`: `ERRORS export errors [unreadable] +
  file errors`. A group exists only to be shared; an error used by one
  method goes in that method's brackets.
- Each error is defined once in the protocol, in the brackets of the one method
  or group that has it. To share an error, put it in a group.
- **Overloads:** methods of a protocol may share a shape and differ in their
  parameters' types, `METHOD tag (paths: LIST Text)` and `METHOD tag (ids:
  LIST File Id)`. A call runs the one whose parameters have its
  arguments' types; see [Annotations and
  overloads](#annotations-and-overloads). Two methods of a shape whose
  parameters have the same types are an error, also when they differ only
  in what they return, since a call states only its arguments' types. (A
  type parameter would overlap every type, should methods ever be
  generic.) `UNDONE BY` writes the types in its call, so it names one
  overload.
- A parameter can't be named `kind`, which JSON uses for a variant's name.

### Refinements

`WHERE` narrows a type with a predicate. It is a type expression, the call
`_ WHERE _`, so it works anywhere a type does: a named `TYPE`, a field, a
parameter, a return, or an argument of a generic type.

```anyx
TYPE Revision: Integer . WHERE [1 <= _],
TYPE Probability: Float . WHERE [0 <= _ <= 1],
TYPE Score Path: Text . WHERE [_ ends with ".poentaro"],
TYPE Limit: (Integer) WHERE [10 <= _ < 30],
RECORD Settings { limit: Limit DEFAULT 10 },
METHOD limit (value: Integer . WHERE [10 <= _ < 30])
    RETURNS (value: Integer . WHERE [10 <= _ < 30]),
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
  bounds, such as `10 <= _ < 30`. Each adjacent pair must hold.
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
- Several predicates separated by commas mean all of them must hold:
  `Text . WHERE [1 <= (_ length), _ ends with ".poentaro"]`.

A predicate must mention `_`. The base type must be concrete enough to check
its operations; an unconstrained type parameter cannot itself be compared
numerically. Refined types can still be passed as generic arguments, or
used in a generic declaration's `LIST` length constraint. Type annotations
compare the predicate's structure, ignoring its whitespace and source spans.
Overloads cannot be distinguished solely by refinements of the same type;
use distinct named `TYPE`s to identify them. Satisfiability, compatibility
checking, and relations between several method arguments or results are
outside this feature.

The wire and JSON keep the base type's representation. A named refined type
still needs its annotation on a request argument, as any named type does:
`write ("a.poentaro" AS: Score Path) at revision (1 AS: Revision)`. An inline
refinement uses that whole type in `AS:` too. Generated clients write these
annotations.

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

### Method conditions

A method can declare a `REQUIRES` clause before `RETURNS` and an
`ENSURES` clause after it.

```anyx
METHOD write (path: Text) at revision (revision: Integer) before (limit: Integer)
    REQUIRES [1 <= revision < limit]
    RETURNS (new revision: Integer)
    ENSURES [new revision =: revision + 1],
```

Conditions share the refinement predicate language. They refer to the
method's named arguments, and `ENSURES` may also refer to the named result.
They can compare numbers or lengths, use comparison chains and arithmetic,
and apply the same text tests as refinements. Use parentheses, dot, or
colon to group nested calls. For example, `new revision =: revision + 1`
and `new revision = (revision + 1)` express the same condition. Each test
in the brackets must hold. Use refinement types for restrictions on individual values that apply
wherever those types are used.

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
    RETURNS (account: account),

## This: the server does the whole transfer, or none of it.
METHOD transfer (amount: Integer) cents from (source: Text) to (target: Text)
    RETURNS (transfer: Text)
    ERRORS [short by (shortfall: Integer) cents],
METHOD close (account: Text) RETURNS NOTHING
    ERRORS [still holds (balance: Integer) cents],
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
    ERRORS [missing],

IDEMPOTENT METHOD archive (path: Text) RETURNS NOTHING
    ERRORS [missing]
    UNDONE BY unarchive (path: Text),

IDEMPOTENT METHOD unarchive (path: Text) RETURNS NOTHING ERRORS [missing],

METHOD move (path: Text) to (new path: Text) RETURNS NOTHING
    ERRORS [missing, taken]
    UNDONE BY move (new path: Text) to (path: Text),

## No markers: may change anything, and is never retried on its own.
METHOD append (text: Text) to (path: Text) RETURNS NOTHING ERRORS [missing],

## Can't claim `UNDONE BY`: the recipient has read the document.
METHOD send (path: Text) to (email: Text) RETURNS NOTHING ERRORS [missing],
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
PUBLIC RECORD Diagnostic { line: Integer, message: Text },
PUBLIC ERRORS common errors [missing],

## notes/files.protocol.anyx
USE "./common.protocol.anyx",
PUBLIC ERRORS file errors [trashed] + common errors,
METHOD read (path: Text) RETURNS (text: Text) ERRORS file errors,

## notes/files/export.protocol.anyx
USE "../common.protocol.anyx",
USE "../files.protocol.anyx",
METHOD pdf (path: Text) RETURNS (pdf: Bytes)
    ERRORS [unreadable (diagnostics: LIST Diagnostic)] + file errors,
```

- `USE "relative path"` is the only import form. It includes that file in
  the protocol and makes its `PUBLIC` types and error groups visible in the
  importing file, by their plain names. Private declarations are visible only
  in their own file. All methods in included files join the protocol.
- There is no re-export: importing `files.protocol.anyx` does not make its imports
  visible. Each file must directly import the declarations it uses.
- Every type and error-group name must be unique across the entire protocol,
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
from calls and type/error-group references, and rename duplicate declarations.
Checker diagnostics identify missing imports, private names and collisions.

### The prelude

The built-in types are declared in the prelude,
[`prelude.protocol.anyx`](prelude.protocol.anyx), written in the schema language with a `#`
doc on each. It is read and checked before every schema, and its names are
visible in every file, so a schema can't declare a type of the same name:
`RECORD Integer { … }` is an error.

```anyx
# A whole number of 64 bits, written without a point: `3`, `-1`.
INTRINSIC TYPE Integer,

INTRINSIC CHOICE Duration { day, hour, minute, second, millisecond },

CHOICE Boolean { false, true },

CHOICE (item: Type) ? {
    null,
    value (item),
},
```

- **`INTRINSIC`** marks a declaration whose meaning the implementation
  supplies: the prelude gives its name, its doc and, for `Duration`, its
  units, and the Rust code gives how its values are read, written and
  generated. Only the prelude may use it; `INTRINSIC` in a schema is an
  error. An `INTRINSIC TYPE` has no type after it.
- The prelude also contains ordinary declarations, checked as user types are.
  `Boolean` and postfix `?` are ordinary choices: their variants and generic
  payloads are declared here, without intrinsic Rust type or value cases.
  A variant slot `(item)` abbreviates `(item: item)`.
- The guide's list of built-in types is the prelude's docs, and a
  `Duration`'s units are the prelude's variants.
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
| `Boolean` | `true`, `false` |
| `Instant` | RFC 3339 text in UTC, to the millisecond: `"2026-11-14T06:00:08.000Z"`; read without the fraction too, `"2026-11-14T06:00:08Z"` |
| `Duration` | whole numbers each followed by its unit, the largest unit first: `90 seconds`, `1 hour 30 minutes` |
| `LIST: T` | `[a, b]` |
| `MAP: T` | `{ "hymn" 3, "ode" 1 }`, each key a string, once, in any order; `"key": value` when the value is a word. Written in the order of the keys' code points |
| a `RECORD` | `{ path "a", revision 3 }`, fields in any order; `name: value` when the value is a word |
| a `CHOICE` | the variant's shape filled in: `dark`, `custom hue 0.5` |
| a use of a generic type | as the `RECORD` or `CHOICE`, with its arguments in place of the parameters: `{ items [{ number 7, total 250 }] }` for `Page of (Order)` |

A number's type is how it is written, as in Rust: `3` and `-1` are
`Integer`s, and `3.0`, `1.5` and `1e3` are `Float`s. So every number states
its own type, and an `Integer` where a `Float` goes is an error ("found the
Integer `3`; expected a Float, such as `3.0`"), in a request and in a
schema's `DEFAULT`s and `EXAMPLES` alike. `Float` was called `NUMBER`
before; only the name changed. A `Float` is always 64 bits, since the wire
carries decimal text and a narrower type would only lose precision; a server
narrows the value itself. There is no exact decimal type: write money as an
`Integer` of cents.

A value written as words, a `Boolean`, a `Duration` or a variant, goes in
parentheses in a slot, where it would otherwise join the words around it:
`set (custom hue 0.5) strict (true)`, `wait (5 minutes)`. In a record, such a
value follows a `:`: `{ every: 1 hour 30 minutes }`, `{ "tea": 5 minutes }`.

A `Duration`'s units are the variants of its declaration in the
[prelude](#the-prelude), `day`, `hour`, `minute`, `second` and
`millisecond`, each in the plural for any count but 1: `1 day 1 second`,
`90 seconds`. A reader takes either form for any count; a writer uses the
singular only for 1. Each unit comes
at most once, after the larger ones; a count may be more than the next unit
holds, as in `90 seconds`. A duration is written with every count below the
next unit, and as `0 seconds` when it is empty. Time finer than a millisecond
is dropped, rounding down, for both types.

## Annotations and overloads

`value AS: type` states the type of an argument whose value doesn't show
it: `tag ([] AS: LIST Text)`, `open ("abc" AS: File Id)`. It is the call
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
| `true`, `false` | the unique matching choice, normally `Boolean`; annotate if ambiguous |
| whole numbers each with a unit | `Duration` |
| a list with items | `LIST` of its items' type, when they all have the same one |
| braces of string keys | `MAP` of its values' type, the same way |
| braces of fields | the record of the schema, not generic, that has each field written and is missing none it requires, when only one does |
| a variant | the choice of the schema, not generic, with a variant of its shape, when only one has one |

Anything else has no type of its own: `[]`, `{}`, a value of a generic
type, and a record or variant that several types fit. A `Bytes`, an
`Instant` and a value of a named `TYPE` are written like a string, or like
the type underneath, so their own type is never theirs.

**An argument of a call must have its parameter's type, of its own or by
an annotation**, even when the method has no overloads:

```anyx
read ("abc" AS: File Id),
upload ("TVRoZA==" AS Bytes),
tag ([] AS: LIST Text),
```

Without it, the request is `INVALID REQUEST` and none of its calls run:
"in `id` of `read _`: `"abc"` has the type Text of its own, not `File Id`;
annotate it, as in `read ("abc" AS: File Id)`". So adding an overload never
breaks a call that worked: every call already states the types it depends
on. Only a whole argument is annotated; a value inside one is read by the
type around it, so `AS` inside a value is an error. The schema's
`EXAMPLES` are calls, so they follow these rules too; a `DEFAULT` is read by
its field's type.

A call of a shape with overloads runs the one whose parameters have exactly
its arguments' types. When none has, or an argument has no type of its own,
the request is `INVALID REQUEST`, and the message lists the overloads by
their typed shapes: "in `tag _`: `[]` has no type of its own, so no
overload of `tag _` can be chosen by it; annotate it with the type of the
one to call, as in `tag ([] AS: LIST Text)`; the overloads are `tag
(names: LIST Text)`, `tag (ids: LIST Integer)`". Since two overloads
can't have the same parameter types, no call fits two.

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
ERROR conflict at revision 4 content "…",
ERROR missing,
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
An `Instant` is its text, as on the wire, and a `Duration` its whole
milliseconds: `5400000` for `1 hour 30 minutes`.
A field with a `DEFAULT` that is left out takes its default. An explicitly
supplied value is always checked against the field's type.
`Boolean` and `T?` use ordinary choice objects too: `{ "kind": "true" }`,
`{ "kind": "null" }`, or `{ "kind": "value", "item": "hello" }`. Generated
Rust uses `Boolean` and `Nullable<Item>` enums, and TypeScript uses tagged
unions; neither choice has an intrinsic primitive representation.
A `MAP` is an object whose keys are kept as written, not camelCased:
`{ "old ode": 1 }`.
A result is `{ "kind": "value", "value": … }` (with no `value` for a method
that returns nothing), `{ "kind": "error",
"error": … }`, `{ "kind": "notRun" }`, `{ "kind": "internalError" }` or
`{ "kind": "invalidRequest", "line": …, "column": …, "message": … }`.

JSON names a method by its full name, `files/read _`. JSON can't tell
`3` from `3.0`, nor a `File Id` from a `Text`, so an overload is named by
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
