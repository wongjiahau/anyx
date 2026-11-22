# poentaro (anyx protocol, version 1)

Send a request as the body of `POST https://example.com/agent`.

## Requests

A request is a list of calls separated by commas. Each call is written in the
shape of one of the methods below, with a value in each `(name: Type)` slot:

    write "Hymns/a.poentaro" at revision 3 content "title \"Ode\""

Every call runs, in order, and the response has one result per call.

`SCHEMA` is a call every server answers, with this guide, and
`SCHEMA FILES` with the files of the schema, the entry first.

## Effects

Words before `METHOD` say what a call of it may do, and decide when it is
safe to send without asking:

- `READ ONLY METHOD`: it changes nothing, so call it freely. `SCHEMA` and
  `SCHEMA FILES` are read only.
- `IDEMPOTENT METHOD`: calling it again with the same arguments changes
  nothing more, so it is safe to retry when a response was lost. A retry may
  still answer differently, such as with an error saying it is already done,
  because the first call went through.
- `METHOD` alone: it may change anything, and calling it twice may do it
  twice. Don't retry it blindly: check first whether the call went through,
  and ask the person you work for when unsure.

`UNDONE BY` after a method's results is the call that undoes it, with the
method's own arguments in its slots: `UNDONE BY move (new path: Text) to
(path: Text)` means that after `move "a" to "b"`, the call `move "b" to
"a"` takes it back. The state afterwards is equivalent, not always exactly
the same. A method without `UNDONE BY` can't be undone by a call: its
effect may be permanent, or it may have shown something to people outside
the account, such as by sending or sharing it.

## Values

- `Text`: A string: `"some text"`. Escape `\"` and `\\`, and write a line
  break as `\n`. For text with many lines, use a raw string: three quotes,
  the lines, then three quotes on a line of their own. The closing quotes'
  indentation is removed from every line. A raw string has no escapes.

      """
          first line
          second line
          """

- `Bytes`: Raw bytes, such as a file: their base64 in a string,
  `"TVRoZA=="`.
- `Integer`: A whole number of 64 bits, written without a point: `3`, `-1`.
- `Float`: A finite 64-bit float, written with a point or an exponent:
  `1.5`, `3.0`, `1e3`. `3` is an `Integer`, so it doesn't fit a `Float`;
  write `3.0`.
- `Instant`: A point in time, in UTC to the millisecond, as a string:
  `"2026-11-14T06:00:08.000Z"`.
- `Duration`: A length of time, to the millisecond: whole numbers each
  followed by its unit in the plural, the largest first, as in `90 seconds`
  or `1 hours 30 minutes`. The units are `days`, `hours`, `minutes`,
  `seconds` and `milliseconds`, and a unit left out counts 0.
- `LIST: T` is `[a, b, c]`.
- `MAP: T` is keys, each a string, with their values in braces:
  `{ "hymn" 3, "ode" 1 }`. Each key appears once. When a value is
  words, put `:` after its key: `` { "hymn": `done } ``.
- A record is its fields in braces, each written in its declared shape with
  a value in each slot, in any order. Declare every slot as
  `(name: Type)`. For example, `quiet from (start: Integer) to (end: Integer)`
  is one field, written `{ quiet from 22 to 7 }`. Other record values include
  `{ path "a", revision 3 }`,
  `` { (`any) case, between 1 and 3 } ``. A value before a field's words
  keeps its parentheses. A field declared as a backtick and a slot is written
  as its variant alone: `` { `by name, `hidden } ``. A field with a `DEFAULT` may
  be left out and then holds the entry after `DEFAULT`. When several fields
  share a default variant, leave those fields out to use it. When a value is words
  and the slot is last, put `:` after the field's words:
  `` { status: `done } ``, `{ timeout: 5 minutes }`.
- A union of variants, `` TYPE ALIAS Shade [| `dark, `light |] ``, is
  one of its variants, written as a backtick and the variant's shape
  with a value in each slot: `` `dark ``, `` `custom hue 0.5 ``. A call
  without the backtick is a function, such as a named type's constructor.
- A union is a value of one of its members, written as that member
  writes it. So a nullable `T?`, `` [| T, `null |] ``, is `` `null `` or a
  value of `T`: `{ max depth 3 }`, `` { max depth: `null } ``.
- In a slot, a value written as words or a variant goes in parentheses so
  that it doesn't join the words around it: `` set (`true) ``,
  `` pick (`custom hue 0.5) ``, `wait (5 minutes)`.

An argument whose type its value doesn't show is annotated with `AS:` and
its parameter's type, in parentheses: `tag ([] AS: LIST Text)`. A value
shows its type when it is a number, a string (`Text`), a duration, a list
or map whose items show one type, a named type's value, written with its
name, or a record or variant that only one type of the schema fits. `[]`,
`{}`, and a value of a generic type, `Bytes` or `Instant` never do.
Without its annotation, the request is invalid. Only a whole argument is
annotated; a value inside it is read by the type around it. An annotation states the type; it doesn't convert the value.

## Prelude

These ordinary types are visible in every schema. `T?` is nullable; only
`DEFAULT` makes a record field omittable.

```anyx
# `` `true `` or `` `false ``.
TYPE ALIAS Boolean [| `false, `true |],

# A `Duration` is written as a call of this function: each unit is an
# optional part, so `1 hours 30 minutes` leaves out the days, seconds and
# milliseconds, which count 0.
INTRINSIC FUNCTION
    [(days: Integer) days DEFAULT 0 days]
    [(hours: Integer) hours DEFAULT 0 hours]
    [(minutes: Integer) minutes DEFAULT 0 minutes]
    [(seconds: Integer) seconds DEFAULT 0 seconds]
    [(milliseconds: Integer) milliseconds DEFAULT 0 milliseconds]
    RETURNS (duration: Duration),

# A nullable item: `` `null ``, or a value of the item written as it is,
# as in `3` for `Integer?`.
TYPE ALIAS (Item: TYPE) ? : [| Item, `null |],

# A file of a schema, as the built-in `SCHEMA FILES` returns it: its path,
# as the schema's `USE` lines reach it, and its source.
TYPE ALIAS Schema File: {
    path (path: Text),
    source (source: Text),
},
```

## Responses

The response has one result per call, in the order of the calls, separated
by commas. A result doesn't repeat its call: the first result answers the
first call, the second the second, and so on, so keep a request short
enough to count its results without slipping. A result is one of:

- `RETURNS` and the value the method returns, or `RETURNS NOTHING` for a
  method that returns nothing.
- `ERROR` and one of the method's errors, in the error's shape: a variant
  in its `ERRORS` brackets, or one of the variants of an alias named there.
  For example: `` ERROR `missing ``. The call didn't do anything. Each
  error's `#` lines say what to do next.
- `INTERNAL ERROR`: the server failed. Calls after it don't run.
- `NOT RUN`: the call didn't run, because an earlier call failed that way.

For two calls, one that read a file and one whose file was missing:

    RETURNS { path "a", revision 3 },
    ERROR `missing,

A request that can't be read gets one result, and none of its calls run:

    INVALID REQUEST { line 1, column 6, message "…" }

## The schema

In the schema, a `#` line before a definition says what it means. Types
are named in Title Case, such as `File Summary`; other names are lowercase
words; all-caps words belong to the schema language. Errors that methods
share are an alias of a union of variants, such as `` TYPE ALIAS File
Errors: [| `missing |] ``, named in each method's `ERRORS`.
`EXAMPLES […]` after a method lists calls of it, each with its result side
by side, as in `read "a" RETURNS …`, and a `#` line saying what it shows.
A response writes the result alone.

# Poentaro turns `.poentaro` text into numbered musical notation (jianpu) and
# keeps the account owner's scores in the cloud. When you save, anyone who
# has the score open sees the change.
#
# Read https://poentaro.com/syntax.md before you write a score.
#
# To edit a score: read it, change the text, then write it at the revision
# you read. If the write comes back as a conflict, apply your change again
# to the content it returns and write at the revision it returns.
PROTOCOL poentaro VERSION 1,

# A file in the account's tree.
TYPE ALIAS File Summary {
    # A slash path, such as `Hymns/hymn.poentaro`.
    path (path: Text),
    revision (revision: Integer),
},

TYPE ALIAS File { path (path: Text), revision (revision: Integer), content (content: Text) },

# One problem in a score.
TYPE ALIAS Diagnostic {
    # Counted from 1, in the content you sent.
    line (line: Integer),
    # Counted from 1, in characters.
    column (column: Integer),
    code (code: Text),
    message (message: Text),
},

TYPE ALIAS File Errors [|
    # The owner moved this file to the trash. Ask them to restore it.
    `trashed,
    # No file has this path. It may have been renamed: run `list files` to
    # see the exact paths.
    `missing,
|],

TYPE ALIAS Score Errors [|
    # The score doesn't parse. Nothing was saved.
    `rejected (diagnostics: LIST Diagnostic),
|],

# Every file this account owns.
READ ONLY METHOD list files
    RETURNS (files: LIST File Summary)
    ERRORS [| |],

READ ONLY METHOD read (path: Text) RETURNS (file: File) ERRORS [| File Errors |],

# Saves the score, if the file is still at `revision`.
METHOD write (path: Text) at revision (revision: Integer) content (content: Text)
    RETURNS (new revision: Integer)
    ERRORS [|
        # Someone saved first. Apply your change to `content`, then write at
        # `revision`.
        `conflict at revision (revision: Integer) content (content: Text),
        Score Errors,
        File Errors,
    |]
    EXAMPLES [
        # Saving over the revision you read.
        write "Hymns/a.poentaro" at revision 3 content "title \"Ode\"" RETURNS 4,
        # Someone saved first.
        write "Hymns/a.poentaro" at revision 3 content "title \"Ode\"" ERROR `conflict at revision 4 content "title \"Hymn\"",
    ],

# Makes a new file, and its folders if they don't exist.
METHOD create (path: Text) content (content: Text)
    RETURNS (revision: Integer)
    ERRORS [|
        # A file already has this path. Read it, or create under another path.
        `already exists at revision (revision: Integer),
        # The path isn't valid, such as an empty folder name or a `..`.
        `invalid path (message: Text),
        Score Errors,
    |],

# The score as a Standard MIDI File.
READ ONLY METHOD export midi (path: Text)
    RETURNS (midi: Bytes)
    ERRORS [| File Errors |],
