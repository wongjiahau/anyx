# The anyx protocol

An app declares its methods in a schema written in anyx. A request is an
anyx document of calls in the methods' shapes; the response is an anyx
document of their results. Both are checked against the schema.

## The schema

```anyx
# What the protocol is for, for whoever reads it first.
PROTOCOL poentaro VERSION 1,

# A file in the account's tree.
RECORD file summary {
    # A slash path, such as `Hymns/hymn.poentaro`.
    path: TEXT,
    revision: INTEGER,
    note: OPTIONAL: TEXT,
},

CHOICE shade { dark, light },

CHOICE colour { dark, light, custom hue (hue: NUMBER), (base: shade) dimmed },

# Which file, among every account's.
TYPE file id: TEXT,

ERRORS file errors [
    # The owner moved this file to the trash.
    trashed,
    missing,
    (path: TEXT) would be both file and folder,
],

# Saves the score, if the file is still at `revision`.
METHOD write (path: TEXT) at revision (revision: INTEGER) content (content: TEXT)
    RETURNS (new revision: INTEGER)
    ERRORS [
        # Someone saved first. Apply your change to `content`, then write at `revision`.
        conflict at revision (revision: INTEGER) content (content: TEXT),
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
  starts a comment.
- **Names** are lowercase words: `file summary`. All-caps words belong to the
  schema language. Keywords side by side join into one name, so write
  `LIST: TEXT` and `OPTIONAL: TEXT` with the `:`.
- **Types:** `TEXT`, `BYTES` (such as a file), `INTEGER` (64-bit), `NUMBER` (a finite float),
  `BOOLEAN`, `INSTANT` (a point in time), `DURATION` (a length of time),
  `LIST: T`, `MAP: T` (text keys, each to a `T`), the name of a
  `RECORD`, `CHOICE` or `TYPE`, and a use of a generic type,
  `page WITH (item: order)`. A type may hold itself only through a
  `LIST` or a `MAP`, which may be empty.
- **`TYPE name: T`** names a type, `TYPE file id: TEXT`, so that values that
  share a representation but mean different things can't be mixed up. It is
  written exactly like `T`, on the wire and in JSON, and generated code
  keeps it distinct from `T` and from other named types over `T`. `T` is any
  type, `TYPE tags: LIST: TEXT`, but can't lead back to the named type.
- **`RECORD name { field: TYPE, … }`**: fields by name. An `OPTIONAL` field
  may be left out. `DEFAULT value` after its type gives it the value it
  holds when left out: `limit: OPTIONAL: INTEGER DEFAULT 50`, or
  `tags: OPTIONAL: LIST: TEXT DEFAULT []`. The value is written as on the
  wire and checked against the type. Only an `OPTIONAL` field of a `RECORD`
  takes a `DEFAULT`; a parameter is always written. The reader of a value
  fills the default in, so code sees the field as always there.
- **`CHOICE name { variant, … }`**: one of its variants. A variant is a
  *shape*: words, with `(name: TYPE)` slots anywhere among them, as in
  `custom hue (hue: NUMBER)` or `(base: shade) dimmed`. Variants differ by
  their words.
- **`GENERIC (parameter, …) RECORD`** or **`CHOICE`** declares a generic
  type, whose fields or variants use each parameter as a type. A use
  gives every parameter a type, by name, in any order:
  `page WITH (item: order)`. A value of it is written exactly like the
  record or choice with those types in place of the parameters, on the
  wire and in JSON, and checked against them, in `DEFAULT`s and
  `EXAMPLES` too.

  ```anyx
  # A page of a long list, and where the next one starts.
  GENERIC (item) RECORD page {
      items: LIST: item,
      next: OPTIONAL: TEXT,
  },

  # What came of a payment, or of any attempt.
  GENERIC (value, problem) CHOICE outcome {
      done (value: value),
      failed (problem: problem),
  },

  METHOD orders RETURNS (orders: page WITH (item: order)),
  METHOD pay (order: order id)
      RETURNS (outcome: outcome WITH (value: receipt, problem: decline)),
  METHOD pay all (orders: LIST: order id)
      RETURNS (outcomes: page WITH (item: outcome WITH (value: receipt, problem: decline))),
  TYPE orders: page WITH (item: order),
  ```

  Only a `RECORD` or a `CHOICE` is generic, and `PUBLIC GENERIC (…) RECORD`
  shares one. A parameter is a lowercase name, once in its declaration,
  used by at least one field or variant, and not the name of a type
  visible there: a type of its scope, or another scope's `PUBLIC` type.
  A parameter takes no arguments, and a field whose type holds one takes
  no `DEFAULT`, since it couldn't be checked against every argument. A
  generic type is always used `WITH` all its arguments, and only a generic
  type takes them. Within its own declaration, a generic type uses itself
  with its own parameters as they are, `tree WITH (item: item)`, and only
  through a `LIST` or a `MAP`. After `LIST`, `MAP` or `OPTIONAL`, a use
  needs the `:`, `LIST: page WITH (item: order)`, since keywords side by
  side join into one name.
- **`METHOD shape RETURNS (name: TYPE) ERRORS errors`**: the method's
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
  `[conflict at revision (revision: INTEGER)] + file errors`, or a group
  alone, `file errors`.
- **`ERRORS name [errors]`** defines a group of errors that methods share.
  It may join other groups with `+`: `ERRORS export errors [unreadable] +
  file errors`. A group exists only to be shared; an error used by one
  method goes in that method's brackets.
- Each error is defined once in a scope, in the brackets of the one method
  or group that has it. To share an error, put it in a group.
- A parameter can't be named `kind`, which JSON uses for a variant's name.

### Writing shapes

A shape is read as a sentence: in a call, an error and the guide. Write it
so the sentence says what a `#` doc would, and keep `#` for what no shape
can say.

- **Put each slot where its word goes**, at the start, middle or end:
  `(path: TEXT) would be both file and folder`, `(email: TEXT) is not a
  test account`, `move (path: TEXT) to (new path: TEXT)`. On the wire it
  reads `"Hymns/a.poentaro" would be both file and folder`.
- **Words after a slot give its unit**, and the slot's name says what the
  value is: `locked for (duration: INTEGER) seconds` reads `locked for 30
  seconds`. Don't repeat the unit as the name, as in `(seconds: INTEGER)
  seconds`.
- **Don't write a `#` doc that restates the shape**, like `# The path would
  be both a file and a folder.` above `(path: TEXT) would be both file and
  folder`. Keep a doc for a rule the shape can't carry, such as
  `# Nothing moved.`
- **A shape whose slot is a sentence keeps it last**: `weak password
  (message: TEXT)`, since no words could follow a message well.
- Generated code names a variant by its words alone, so
  `locked for (duration: INTEGER) seconds` is `LockedForSeconds { duration }`.
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
METHOD update account (id: TEXT) fields (fields: account fields)
    RETURNS (account: account),

## This: the server does the whole transfer, or none of it.
METHOD transfer (amount: INTEGER) cents from (source: TEXT) to (target: TEXT)
    RETURNS (transfer: TEXT)
    ERRORS [short by (shortfall: INTEGER) cents],
METHOD close (account: TEXT) RETURNS NOTHING
    ERRORS [still holds (balance: INTEGER) cents],
```

- **The shape names the intent.** `transfer (amount) cents from (source) to
  (target)` reads as what the caller means; `update account (id) fields
  (fields)` says only what it touches.
- **Reads may be uniform.** Listing and filtering look alike for every
  record, so `list accounts (filter: account filter)` is fine. Keep writes
  as intents.

Schemas are formatted with `anyx_protocol::format_schema`, which puts a long
`METHOD`'s `RETURNS`, `ERRORS`, `UNDONE BY` and `EXAMPLES` on lines of their
own.

### Effects

Words around a method say what a call of it does to the world, so that a
client can decide on its own whether to retry a call whose response was
lost, and whether to ask a person before sending it.

```anyx
READ ONLY METHOD read (path: TEXT) RETURNS (document: document)
    ERRORS [missing],

IDEMPOTENT METHOD archive (path: TEXT) RETURNS NOTHING
    ERRORS [missing]
    UNDONE BY unarchive (path: TEXT),

IDEMPOTENT METHOD unarchive (path: TEXT) RETURNS NOTHING ERRORS [missing],

METHOD move (path: TEXT) to (new path: TEXT) RETURNS NOTHING
    ERRORS [missing, taken]
    UNDONE BY move (new path: TEXT) to (path: TEXT),

## No markers: may change anything, and is never retried on its own.
METHOD append (text: TEXT) to (path: TEXT) RETURNS NOTHING ERRORS [missing],

## Can't claim `UNDONE BY`: the recipient has read the document.
METHOD send (path: TEXT) to (email: TEXT) RETURNS NOTHING ERRORS [missing],
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
  call of another method of the same scope. It is written like a shape whose
  slots are the method's own parameters, each `(name: TYPE)` with that
  parameter's name and type: `move (path) to (new path)` is undone by
  `move (new path: TEXT) to (path: TEXT)`. The types say which method it
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

### Scopes

A schema can be a folder instead of one file. `protocol.anyx` at its root
holds only the `PROTOCOL` header, and every other `.anyx` file is a *scope*,
named by its path without `.anyx`:

```text
notes/
  protocol.anyx        PROTOCOL notes VERSION 1,
  common.anyx          scope `common`
  files.anyx           scope `files`
  files/export.anyx    scope `files/export`
```

A file or folder name is one lowercase word, letters and digits.

- **A method is called by its full name**: the scope's path, `/`, then its
  shape. `METHOD pdf (path: TEXT)` in `files/export.anyx` is called as
  `files/export/pdf "a.note"`. A method's own shape has no `/`.
- **Types and error groups are private** to the scope that declares them;
  folders give no special access, so `files/export` can't see what `files`
  keeps to itself. **`PUBLIC`** before `RECORD`, `CHOICE`, `TYPE` or
  `ERRORS` shares it, and another scope then names it by its full path:

```anyx
## common.anyx
PUBLIC RECORD diagnostic { line: INTEGER, message: TEXT },
PUBLIC ERRORS common errors [missing],

## files.anyx
PUBLIC ERRORS file errors [trashed] + common/common errors,
METHOD read (path: TEXT) RETURNS (file: file) ERRORS file errors,

## files/export.anyx
METHOD pdf (path: TEXT) RETURNS (pdf: BYTES)
    ERRORS [unreadable (diagnostics: LIST common/diagnostic)] + files/file errors,
```

- In its own scope, a type or group is named without its path.
- An error is written on the wire by its words alone, `ERROR missing`, so a
  method can't list two errors of the same words from different scopes.

## Requests

A request is calls separated by commas, each in a method's shape with a value
in each slot:

```anyx
list files,
write "Hymns/a.poentaro" at revision 3 content """
    title "Ode"
    """,
```

In a schema of scopes, each call starts with its scope's path:
`files/export/pdf "a.note", files/list`. Spaces around the `/` are allowed.

Every call runs, in order.

`SCHEMA` is a built-in call every server answers, with no code of its own:
it returns the guide (below) as `TEXT`, without how to send a request,
since the caller just sent one. A client that has only a server's address
learns everything else from it:

```anyx
SCHEMA,
```

## Values

| Type | Written |
| --- | --- |
| `TEXT` | `"text"` with `\"`, `\\`, `\n`, `\r`, `\t` and `\u{…}` escapes; or a raw string of three or more quotes, whose closing quotes' indentation is removed from every line |
| `BYTES` | its standard base64 (RFC 4648, padded) as a string: `"TVRoZA=="` |
| `INTEGER`, `NUMBER` | `3`, `-1.5` |
| `BOOLEAN` | `true`, `false` |
| `INSTANT` | RFC 3339 text in UTC, to the millisecond: `"2026-11-14T06:00:08.000Z"`; read without the fraction too, `"2026-11-14T06:00:08Z"` |
| `DURATION` | whole numbers each followed by its unit, the largest unit first: `90 seconds`, `1 hour 30 minutes` |
| `LIST: T` | `[a, b]` |
| `MAP: T` | `{ "hymn" 3, "ode" 1 }`, each key a string, once, in any order; `"key": value` when the value is a word. Written in the order of the keys' code points |
| a `RECORD` | `{ path "a", revision 3 }`, fields in any order; `name: value` when the value is a word |
| a `CHOICE` | the variant's shape filled in: `dark`, `custom hue 0.5` |
| a use of a generic type | as the `RECORD` or `CHOICE`, with its arguments in place of the parameters: `{ items [{ number 7, total 250 }] }` for `page WITH (item: order)` |

A value written as words, a `BOOLEAN`, a `DURATION` or a variant, goes in
parentheses in a slot, where it would otherwise join the words around it:
`set (custom hue 0.5) strict (true)`, `wait (5 minutes)`. In a record, such a
value follows a `:`: `{ every: 1 hour 30 minutes }`, `{ "tea": 5 minutes }`.

A `DURATION`'s units are `days`, `hours`, `minutes`, `seconds` and
`milliseconds`, each in the singular for 1: `1 day 1 second`. Each unit comes
at most once, after the larger ones; a count may be more than the next unit
holds, as in `90 seconds`. A duration is written with every count below the
next unit, and as `0 seconds` when it is empty. Time finer than a millisecond
is dropped, rounding down, for both types.

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
An `INSTANT` is its text, as on the wire, and a `DURATION` its whole
milliseconds: `5400000` for `1 hour 30 minutes`.
A field with a `DEFAULT` that is left out or `null` takes its default.
A `MAP` is an object whose keys are kept as written, not camelCased:
`{ "old ode": 1 }`.
A result is `{ "kind": "value", "value": … }` (with no `value` for a method
that returns nothing), `{ "kind": "error",
"error": … }`, `{ "kind": "notRun" }`, `{ "kind": "internalError" }` or
`{ "kind": "invalidRequest", "line": …, "column": …, "message": … }`.

## The guide

`anyx_protocol::guide(schema, how_to_send)` writes the markdown an agent
reads before its first request, and what `SCHEMA` answers (with
`how_to_send` empty): these rules, briefly, then the schema with
its `#` lines, a section per scope's file. Its sample call is the first
example of the widest method with `EXAMPLES`, or, in a schema without
any, one it makes up with a sample value in each slot.
