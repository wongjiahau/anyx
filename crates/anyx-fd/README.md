# anyx-fd

`fd`, finding files by name, as an anyx core tool. It is written against the
libraries `fd` is built on (`ignore`, `globset` and `regex`), not by running
`fd`. Its schema is [`fd.anyx`](fd.anyx); `build.rs` generates the server
from it with `anyx-core-generate`, and `src/lib.rs` implements the three
functions it serves.

```sh
cargo install --path crates/anyx-fd
```

In the anyx shell:

```anyx
USE fd FROM COMMAND "anyx-fd",
fd / find "\\.rs$" in ["src"] with { only [file], .hidden },
fd / details of "Cargo" with { .max depth 2 },
fd / any match of "TODO",
```

`find` returns paths, `details of` returns a record for each path, and
`any match of` returns whether anything matched. Each takes two optional
parts: `in (paths)`, which defaults to the working directory, and
`with (search)`, which defaults to fd's own defaults. A search is a
record of `fd`'s flags, each written as it reads: `hidden`, `glob pattern`,
`any case`, `at least 14 bytes`, `changed within (1 hours)`. Each field comes
at most once, so a search can't contradict itself: the pattern's syntax and
case are each one choice.

Each answer is an `Outcome`: `answered ["src/lib.rs"]`, or a failure such as
`failed (invalid pattern "unclosed group")`.

As in `fd`, the pattern is a regular expression matched against the file
name, in smart case, skipping hidden and ignored files. Results are sorted by
name within each search path. A path that isn't Unicode fails with
`(path) is not unicode`.

Some flags have nothing to do here because results are typed values (`-0`,
`--format`, `--color`), and the shell replaces others: nested calls replace
`-x`/`-X`, and the working directory replaces `--base-directory`.

## Protocol

`anyx-fd --anyx-schema` writes the schema. `anyx-fd --anyx-protocol` reads
one request, a JSON list of calls of the entry point, on standard input,
writes the answers on standard output, and exits:

```sh
anyx-fd --anyx-schema
echo '[{"function": "answer _ to _", "arguments": {"request": {"kind": "any match of in with", "pattern": "lib", "paths": [], "search": …}}}]' \
  | anyx-fd --anyx-protocol
```

A request is checked against the schema before it is sent, so the shell
writes every field of `search`, defaults included.
