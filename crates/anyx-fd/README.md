# anyx-fd

`fd`, finding files by name, as an anyx protocol tool. It is written against
the libraries `fd` is built on (`ignore`, `globset` and `regex`), not by
running `fd`. Its schema is [`fd.protocol.anyx`](fd.protocol.anyx).

```sh
cargo install --path crates/anyx-fd
```

In the anyx shell:

```anyx
USE fd FROM COMMAND "anyx-fd",
fd / find "\\.rs$" in ["src"] with { only [file], hidden },
fd / details of "Cargo" with { max depth 2 },
fd / any match of "TODO",
```

`find` returns paths, `details of` returns a record for each path, and
`any match of` returns whether anything matched. Each has four shapes: with
or without `in (paths)`, and with or without `with (search)`. A search is a
record of `fd`'s flags, each written as it reads: `hidden`, `(glob) pattern`,
`(any) case`, `at least 14 bytes`, `changed within (1 hour)`. Each field comes
at most once, so a search can't contradict itself: the pattern's syntax and
case are each one choice.

As in `fd`, the pattern is a regular expression matched against the file
name, in smart case, skipping hidden and ignored files. Results are sorted by
name within each search path. A path that isn't Unicode is the error
`(path) is not unicode`.

Some flags have nothing to do here because results are typed values (`-0`,
`--format`, `--color`), and the shell replaces others: nested calls replace
`-x`/`-X`, and the working directory replaces `--base-directory`.

## Protocol

`anyx-fd --anyx-protocol` reads one request on standard input, writes the
response on standard output, and exits:

```sh
echo 'find "lib" with { only [file] }' | anyx-fd --anyx-protocol
echo 'SCHEMA' | anyx-fd --anyx-protocol
```
