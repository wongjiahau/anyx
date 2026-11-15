# notes (anyx protocol, version 1)

Send a request as the body of `POST /notes`.

## Requests

A request is a list of calls separated by commas. Each call is written in the
shape of one of the methods below, with a value in each `(name: TYPE)` slot:

    files/write "a" at revision 3 content "a"

A method in a scope is called by its full name, as above: the scope's
path, `/`, then the call.

Every call runs, in order, and the response has one result per call.

`SCHEMA` is a call every server answers, with this guide.

## Effects

Words before `METHOD` say what a call of it may do, and decide when it is
safe to send without asking:

- `READ ONLY METHOD`: it changes nothing, so call it freely. `SCHEMA` is
  read only.
- `IDEMPOTENT METHOD`: calling it again with the same arguments changes
  nothing more, so it is safe to retry when a response was lost. A retry may
  still answer differently, such as with an error saying it is already done,
  because the first call went through.
- `METHOD` alone: it may change anything, and calling it twice may do it
  twice. Don't retry it blindly: check first whether the call went through,
  and ask the person you work for when unsure.

`UNDONE BY` after a method's results is the call that undoes it, with the
method's own arguments in its slots: `UNDONE BY move (new path: TEXT) to
(path: TEXT)` means that after `move "a" to "b"`, the call `move "b" to
"a"` takes it back. The state afterwards is equivalent, not always exactly
the same. A method without `UNDONE BY` can't be undone by a call: its
effect may be permanent, or it may have shown something to people outside
the account, such as by sending or sharing it.

## Values

- TEXT is a string: `"some text"`. Escape `\"` and `\\`, and write a line
  break as `\n`. For text with many lines, use a raw string: three quotes,
  the lines, then three quotes on a line of their own. The closing quotes'
  indentation is removed from every line. A raw string has no escapes.

      """
          first line
          second line
          """

- BYTES is base64 in a string: `"TVRoZA=="`.
- INTEGER is a whole number, written without a point: `3`, `-1`.
- FLOAT is a 64-bit float, written with a point or an exponent: `1.5`,
  `3.0`, `1e3`. `3` is an INTEGER, so it doesn't fit a FLOAT; write `3.0`.
- BOOLEAN is `true` or `false`.
- INSTANT is a point in time, in UTC to the millisecond, as a string:
  `"2026-11-14T06:00:08.000Z"`.
- DURATION is whole numbers each followed by its unit, the largest first:
  `90 seconds`, `1 hour 30 minutes`. The units are `days`, `hours`,
  `minutes`, `seconds` and `milliseconds`, or `day`, `hour` and so on for 1.
- `LIST: T` is `[a, b, c]`.
- `MAP: T` is keys, each a string, with their values in braces:
  `{ "hymn" 3, "ode" 1 }`. Each key appears once. When a value is
  words, put `:` after its key: `{ "hymn": done }`.
- A RECORD is its fields in braces, each its name and value, in any order:
  `{ path "a", revision 3 }`. An `OPTIONAL` field may be left out; one
  with a `DEFAULT` then holds that value. When a value is words, put `:`
  after the field's name: `{ status: done }`, `{ timeout: 5 minutes }`.
- A CHOICE is one of its variants, written in the variant's shape with a
  value in each slot: `dark`, `custom hue 0.5`.
- In a slot, a value written as words goes in parentheses so that it doesn't
  join the words around it: `set (true)`, `pick (custom hue 0.5)`,
  `wait (5 minutes)`.

An argument whose type its value doesn't show is annotated with `AS:` and
its parameter's type, in parentheses: `tag ([] AS: LIST: TEXT)`,
`open ("a" AS: file id)`. A value shows its type when it is a number, a
string (TEXT), `true` or `false`, a duration, a list or map whose items
show one type, or a record or variant that only one type of the schema
fits. `[]`, `{}`, and a value of a named TYPE, a generic type, BYTES or
INSTANT never do. Without its annotation, the request is invalid. Only a
whole argument is annotated; a value inside it is read by the type around
it. An annotation states the type; it doesn't convert the value.

## Responses

The response has one result per call, in the order of the calls, separated
by commas. A result doesn't repeat its call: the first result answers the
first call, the second the second, and so on, so keep a request short
enough to count its results without slipping. A result is one of:

- `RETURNS` and the value the method returns, or `RETURNS NOTHING` for a
  method that returns nothing.
- `ERROR` and one of the method's errors, in the error's shape. They follow
  `ERRORS`: its own in brackets, then those of each group joined with `+`.
  For example: `ERROR missing`. The call didn't do anything. Each error's
  `#` lines say what to do next.
- `INTERNAL ERROR`: the server failed. Calls after it don't run.
- `NOT RUN`: the call didn't run, because an earlier call failed that way.

For two calls, one that read a file and one whose file was missing:

    RETURNS { path "a", revision 3 },
    ERROR missing,

A request that can't be read gets one result, and none of its calls run:

    INVALID REQUEST { line 1, column 6, message "…" }

## The schema

In the schema, a `#` line before a definition says what it means. Lowercase
words are names; all-caps words belong to the schema language. `ERRORS name
[…]` is a group of errors that methods share by joining it with `+`.

The schema is a file per scope, each shown below under its full name. A
type or an error group is private to its scope unless it is `PUBLIC`; another
scope writes its full path, such as `common/diagnostic`. A response writes
an error in its shape alone, without its scope: `ERROR missing`.

### `protocol.anyx`

# Notes keeps a person's notes, a file each.
PROTOCOL notes VERSION 1,

### `common` (`common.anyx`)

# One problem in a note.
PUBLIC RECORD diagnostic { line: INTEGER, message: TEXT },

PUBLIC ERRORS common errors [
    # No note has this path.
    missing,
],

### `files` (`files.anyx`)

Methods: `files/list`, `files/read _`, `files/write _ at revision _ content _`, `files/trash _`, `files/empty trash`.

RECORD file { path: TEXT, revision: INTEGER, content: TEXT },

PUBLIC ERRORS file errors [
    # The note is in the trash.
    trashed,
] + common/common errors,

READ ONLY METHOD list RETURNS (paths: LIST: TEXT),

READ ONLY METHOD read (path: TEXT) RETURNS (file: file) ERRORS file errors,

# Saves the note, if it is still at `revision`.
METHOD write (path: TEXT) at revision (revision: INTEGER) content (content: TEXT)
    RETURNS (warnings: LIST common/diagnostic)
    ERRORS [
        # The note has errors, so nothing was saved.
        rejected (diagnostics: LIST common/diagnostic),
    ] + file errors,

# Moves the note to the trash.
IDEMPOTENT METHOD trash (path: TEXT) RETURNS NOTHING ERRORS file errors,

# Forgets every note in the trash.
METHOD empty trash RETURNS NOTHING,

### `files/export` (`files/export.anyx`)

Methods: `files/export/html _`, `files/export/pdf _`.

# A page of a note, with what is wrong on it.
RECORD page { html: TEXT, problem: OPTIONAL common/diagnostic },

METHOD html (path: TEXT) RETURNS (pages: LIST page) ERRORS files/file errors,

METHOD pdf (path: TEXT) RETURNS (pdf: BYTES)
    ERRORS [
        # The note has errors, so it can't be exported.
        unreadable (diagnostics: LIST common/diagnostic),
    ] + files/file errors,
