# TODO: the anyx parser generator

A parser generator whose grammar language is anyx, described in itself.
It reads a grammar and writes Rust: a typed AST, plus a decoder that turns
the tree from `SyntaxTree::nodes()` into that AST.

## 1. Workspace

- [x] Move today's crate to `crates/anyx`, with a root `Cargo.toml` workspace
- [x] Create `crates/anyx-generator` (library + command line)
- [x] Keep `cargo test`, `cargo clippy` and lefthook working from the root

## 2. The grammar language

- [x] Put the grammar of grammars in `crates/anyx-generator/grammar.grammar.anyx`
- [x] Put the JSON grammar in `crates/anyx-generator/tests/fixtures/json.grammar.anyx`
- [x] Write the grammar language reference (README), covering:
  - [x] `ENTRY` first, then rules: `name: pattern`, lowercase names
  - [x] Rule references: bare lowercase words
  - [x] `ONE OF [...]`: first match wins
  - [x] `ZERO OR MORE:`, `ONE OR MORE:`, `OPTIONAL:`
  - [x] `CURLY BRACE`, `SQUARE BRACKET`, `PARENTHESIS`, with `: pattern` or `[elements in order]`
  - [x] `ANYFIX CALL [WORD "IF", condition: expression, ...]`
  - [x] `ANYFIX CALL [NAME [...], ARGUMENTS: pattern]`
  - [x] `WORD "x"`, `ANY WORD [KEYWORD | NAME | SYMBOL | string conditions]`
  - [x] `STRING`, `STRING "exact"`, `STRING [STARTS WITH, ENDS WITH, CONTAINS, MATCHING REGEX]`
  - [x] `NUMBER`, `NUMBER 2`, `NUMBER [INTEGER, AT LEAST n, AT MOST n]`
  - [x] Names: `name: pattern` names a field or a choice. A rule reference defaults to the rule's name, and `WORD "null"` defaults to `Null`

## 3. Handwritten decoder for the grammar language

- [x] Typed AST for `grammar.grammar.anyx`, with a span on every node
- [x] Decoder from `anyx::Node` to that AST
- [x] Decode errors with spans: "expected `ONE OF [...]`, found ..."
- [x] Tests: `grammar.grammar.anyx` and `json.anyx` decode, and bad grammars give good errors

## 4. Checking a grammar

- [x] Every referenced rule exists, and every rule is reachable from `ENTRY`
- [x] Every built-in part has a name (`key: STRING`), and no two fields or choices share a name
- [x] Names are valid as Rust identifiers once converted
- [x] Warn when two `ONE OF` choices can match the same input
- [x] Reject left recursion that can't terminate (a rule that only refers to itself)

## 5. Generating Rust

- [x] Struct per sequence or call, enum per `ONE OF`, `Vec` for repetition, `Option` for `OPTIONAL`
- [x] Spans everywhere: `span: Span` on structs, `span()` on enums, `Spanned<T>` for built-ins
- [x] `STRING` → `Spanned<String>`, keeping the `StringLiteral` pieces
- [x] `NUMBER` → type chosen by its conditions (`u8`, `i64`, `f64`), with errors when out of range
- [x] `ANYFIX CALL` without `ARGUMENTS` → raw `anyx::Node` arguments
- [x] Recursive types boxed where needed
- [x] Decoder functions, one per rule, with errors pointing at the source
- [x] Generated code is formatted (rustfmt) and passes clippy

## 6. Command line

- [x] `anyx-generate grammar.grammar.anyx > src/parser.rs`
- [x] Errors and warnings show line:column and the source line
- [x] Output is deterministic, so committed files diff cleanly

## 7. Bootstrapping

- [x] Generate the grammar language's decoder from `grammar.grammar.anyx`
- [x] Replace the handwritten decoder (step 3) with the generated one
- [x] Test: regenerating from `grammar.grammar.anyx` gives the committed file byte for byte

## 8. Examples and tests

- [x] JSON: decode `tests/fixtures/json.anyx` (the anyx crate's) into the generated AST
- [x] A keyword language example (`IF _ THEN _ ELSE _`, user-defined calls via `ANYFIX CALL`)
- [x] Error snapshot tests for decode failures

## 9. Skill for AI agents

- [x] A SKILL that teaches an agent to write a grammar and run the generator
      (`skills/anyx-grammar/SKILL.md`), with `anyx-generate --tree` to
      see how anyx groups a sample before writing rules for it
- [x] Worked examples: JSON, a config format, a keyword language
- [x] Common mistakes and how to read the generator's warnings

## Related changes in anyx

- [x] Negative number literals (`-` before a digit always starts a number)
- [x] Every number has a value (`1mb` is `1 mb`)
- [ ] One-line raw strings, for regexes ([#2](https://github.com/wongjiahau/anyx/issues/2))

## Protocol clients and batching

Typed batches replace `batch(build)`, whose result is one type for every
position. A batch is for a few calls written out by hand; a method whose
result grows with the data is the schema's job, not the batch's.

- [ ] Call values: a function per method builds a typed call, such as
      `files.read(id)` giving `Call<File, ReadError>`. It sends nothing;
      `client.call(c)` sends one and `client.batch([...])` sends several.
      `client.files.read(id)` stays as shorthand for
      `client.call(files.read(id))`. TS and Rust generators both.
- [ ] Typed tuple batch of 1 to 5 calls, returning the matching tuple of
      outcomes, each position its own type. Rust: a trait implemented for
      tuples up to 5. TS: a tuple type of length at most 5. Replaces
      `batch(build)`.
- [ ] A Rust client next to the server: call values, `call`, `batch`, and a
      `Codec` plus `send` like the TS client.
- [ ] Server limit on calls per request (5, matching the client cap): a
      request over it is `INVALID REQUEST` before any call runs. Document it
      in `PROTOCOL.md`.

## Open questions

- [x] Decode a list's elements each on its own, keeping the errors (`EACH:`)
- [ ] A way to say "any node" (no wildcard yet)
- [ ] More conditions for `ANYFIX CALL` and `ANY WORD` as needs come up
- [ ] A `build.rs` library entry point, later
- [ ] Output in languages other than Rust, later

## Protocol tools from other programs: `fd` first, then `rg`

A tool is any program that answers `--anyx-protocol`: the shell writes one
request to its standard input, closes it, and reads the response from its
standard output. The tool is written against the libraries a command is
built on, never by running the command. `fd` comes first because it brings
in the new plumbing with the smallest domain, and `rg` then selects files by
calling it: `rg / search "TODO" in (fd / …)`.

### Protocol

- [x] Check whether a condition accepts a choice's variants as constants,
      `REQUIRES [settings has at most one of [any case, smart case]]`;
      support it if not
- [x] Prelude: `(items: LIST Type) has no repeats` and
      `(items: LIST Type) has at most one of (choices: LIST Type)`, with
      tests and `PROTOCOL.md`
- [x] A built-in call that returns the schema's files, since `SCHEMA`
      returns the guide; choose its name
- [x] Decide how a path that is not Unicode is returned: an error, `Bytes`,
      or a lossy conversion (an error, `(path) is not unicode`, for now)
- [ ] Generic functions in the prelude, in place of the special word `Type`
      in `LIST Type`, which links no use of it to another. Decided:
      - `FOR ANY [ … ]` right after `FUNCTION` declares the type parameters,
        before the shape that uses them, in the `(name: marker)` form of a
        record's slots; the shape refers to them by name. `FOR ANY` won over
        `FOR ALL` (correct, but reads as logic) and `FOR EVERY`; it can't be
        confused with "at least one", as in `any match of`, since it only
        follows `FUNCTION`.
      - Declared up front, not at their first use inside a parameter
        (`(items: LIST: (item: Type))`), since one parameter can be used by
        several parameters.
      - A `Choice` marker beside `Type`, for a parameter that must be a
        `CHOICE`.
      - `VARIANT OF item`, the type of a variant shape with `_` in each slot,
        such as `max depth _`, so the checker types `has at most one of`
        and the special case for it in `refinement.rs` goes.

      ```anyx
      FUNCTION FOR ANY [(item: Type)]
          (items: LIST: item) has no repeats
          RETURNS (holds: Boolean),

      FUNCTION FOR ANY [(item: Choice)]
          (items: LIST: item) has at most one of (variants: LIST: VARIANT OF item)
          RETURNS (holds: Boolean),

      FUNCTION FOR ANY [(item: Type)] (value: LIST: item) length RETURNS (size: Integer),
      ```

### Records whose fields are anyfix shapes, and `@` fields

fd's settings were a `LIST Setting` with a 13-line `REQUIRES` block copied
into six methods. The block spelled out what a record gives by structure
(each field at most once, some options mutually exclusive), but a record
entry had to be `name value`, so a record couldn't read like
`{hidden, (any) case}`.

Decided:
- Every field is a shape whose slots hold types, and a value writes it as
  declared, in any order. There is no separate "named" kind: `path: Text`
  is `path (Text)`, written `path "a"`. The field is named by its words
  (`at least (Size?) bytes` is `at least bytes`).

  ```anyx
  RECORD Search {
      (Case) case DEFAULT (smart) case,
      (Syntax) pattern DEFAULT (regex) pattern,
      hidden,
      max depth (Depth?) DEFAULT max depth (null),
      at least (Size?) bytes DEFAULT at least (null) bytes,
      only (LIST Kind) DEFAULT only [],
  },
  ```
  ```anyx
  fd / find "*.md" in ["docs"] with { (glob) pattern, (any) case, max depth (value 3), hidden }
  ```
  - Words alone are a checkbox: a `Boolean`, true when written, false when
    left out, and never with a `DEFAULT`.
  - A field with several slots holds a tuple, a value for each:
    `between (Integer) and (Integer)`, written `between 1 and 3`, is `[1, 3]`
    in JSON and TypeScript and `(i64, i64)` in Rust. A tuple has no syntax
    of its own.
  - `DEFAULT` takes the whole field, `_ DEFAULT _` at the root, and holds
    the entry the field takes when left out, written as a value writes it:
    `age (Integer) DEFAULT age 0`. A default is read as an entry of its
    field, so one entry covers every slot. With a `:`, the field goes in
    parentheses: `(age: Integer) DEFAULT age 0`.
  - A nullable value is written with its constructor, `max depth (value 3)`:
    no syntax sugar without the owner's approval.
  - A value before the words keeps its parentheses, `(any) case`: `any case`
    is one call. Readability over writability; the decoder hints at it.
- `@(name: T)` is a field written as `@` and its value alone, `@by name`,
  for choices whose variants are self-describing phrases that share no word.
  `@` makes it distinct at the call site from a shaped field, and the
  declaration has the same syntax as the call site. A symbol, not a keyword,
  so it costs one character to write: `UNNAMED` would have had to appear
  at the call site too.
- An `@` entry finds its field by its value's outermost form alone: a
  variant's shape, a string, a number, a list, braces or a duration. No two
  `@` fields may share a form, even one variant's shape, and `Integer` and
  `Float` share the number's form, `Text` and `Instant` the string's. So
  decoding never tries a field and backs out: no graph search. One of a type
  parameter is an error.
- The guideline is in `PROTOCOL.md` under Records: checkbox, separate fields
  for options that combine, a shape by default, the shared word in the
  shape, and `@` only when a word would stutter. fd uses no `@` field.
- The encoder writes each field in its shape, leaves out a `null` default,
  a false checkbox and an `@` field at its default, and writes other
  defaults out. The formatter hugs a leading symbol to a word, string or
  group: `@by name`, `@"src"`, but `@ 3`.
- Mutual exclusion is one choice field: `{ (glob) pattern, (fixed text)
  pattern }` is the duplicate-field error. fd's `exact depth` went: it is
  `{ min depth 2, max depth 2 }`.
- "A needs B" is nesting, A inside B's value. A need that isn't a tree is a
  `REQUIRES`.
- The prelude keeps `has no repeats` and `has at most one of`: fd no longer
  uses them, but they still serve a method that takes a list.

Tried and ruled out:
- `UNNAMED name: T` as a keyword: heavy, and nothing marked the value at the
  call site.
- A parameter in a field's slot, `max depth (depth: Depth?)`: a lowercase
  slot name is ambiguous with a generic's type parameter
  (`other: item of (Text)` read as a parameter). The words name the field,
  so the slot holds a type.
- `DEFAULT` after the shape with a bare value, `(Case) case DEFAULT smart`,
  and then `DEFAULT` in the slot, `(Case DEFAULT smart) case`: a slot's
  default left a field with several slots needing a default on every slot
  or none. The whole field's entry after `DEFAULT` says it once.
- A nullable field written as its item, `max depth 3` for `Depth?`: an
  implicit `value _` that the call tree didn't show.
- Any choice field writable by its variant without a marker, and an
  `@` field found by trying each field's type: both make decoding a search.
- `OPTIONS` as a new kind of type, groups by `|`, `REPEATABLE`, `NEEDS` and
  bundles like fd's `-u`.

- [x] Fields as shapes, checkboxes and `@` in `protocol.grammar.anyx`, with
      the checker's one-field-per-form rule for `@` fields
- [x] Decode, encode and format the literal; the shell's compiler
- [x] Generated TypeScript
- [x] `PROTOCOL.md` and the guide
- [x] Rework `fd.protocol.anyx`: `RECORD Search` in place of `LIST Setting`
      and every `REQUIRES`
- [ ] The shell's completion and hover inside a record's braces

### Shell

- [x] `USE fd FROM COMMAND "anyx-fd"` in `shell.grammar.anyx`, the source a
      choice so that `URL "…"` can follow; `USE ALL FROM` keeps taking a name
- [x] Ask for the schema on the `USE` line, and report a missing program or a
      broken schema there, before any call runs
- [ ] Run a request: start the program with `--anyx-protocol` in the shell's
      working directory, write the request, close standard input, read the
      response; standard error goes to the log (done, except the log: the
      shell has none yet, so standard error is shown only when `USE` fails)
- [x] A program that exits with a failure, is killed, or answers something
      unreadable gives `INTERNAL ERROR`
- [x] Check the response against the schema before its values reach another
      call
- [x] Completion and argument hints from the fetched schema

### The `fd` tool (`crates/anyx-fd`, program `anyx-fd`)

- [x] Account for every flag of `fd` 10.2:
  - sentence parts: the pattern and the search paths
  - settings, `with [hidden, follow links, "*.pyc" excluded, …]`: `-H`,
    `-I`, `--no-ignore-vcs`, `--no-ignore-parent`, `--no-require-git`,
    `-u`, `-s`/`-i` (smart case by default), `-g`/`--regex`, `-F`, `-p`,
    `-L`, `-d`/`--min-depth`/`--exact-depth`, `-E`, `--prune`, `-t`, `-e`,
    `-S`, `--changed-within`/`--changed-before`, `-o`, `--ignore-file`,
    `--one-file-system`, `--and`, `--max-results`
  - methods of their own: `-l` (files with details, another result type),
    `-q` (`any file …`, a `Boolean`)
  - nothing to do, since results are typed values: `-a`, `-0`, `-c`,
    `--hyperlink`, `--format`, `--path-separator`, `--strip-cwd-prefix`,
    `--show-errors` (unreadable folders are a field of the result)
  - replaced by the shell: `-x`/`-X`/`--batch-size` by nested calls,
    `--base-directory` by the working directory, `--search-path` by the
    paths
  - left to the tool: `-j`
- [x] `fd.protocol.anyx`: the methods, the settings choice, its `REQUIRES`,
      the errors (bad pattern, missing path), `READ ONLY`, `EXAMPLES`
- [x] Implement with `ignore::WalkBuilder`, `globset` and `regex`
- [x] Tests: the schema loads, its examples hold, conflicting settings are
      `INVALID REQUEST`, and a session runs `USE fd FROM COMMAND "anyx-fd"`
      end to end

### The `rg` tool, after `fd`

- [ ] `rg.protocol.anyx`: `search _` and `search _ in _`, settings for how a
      line matches, a method for each kind of result (lines, counts, files
      with matches, replacements), pages of matches
- [ ] Implement with `grep-searcher`, `grep-regex` and `grep-matcher`
- [ ] The demo: `rg / search "TODO" in (fd / …)`
