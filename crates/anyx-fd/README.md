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
fd / find "\\.rs$" in ["src"] with [(file) only, hidden],
fd / details of "Cargo" with [max depth 2],
fd / any match of "TODO",
```

`find` returns paths, `details of` returns a record for each path, and
`any match of` returns whether anything matched. Each has four shapes: with
or without `in (paths)`, and with or without `with (settings)`. The settings
are `fd`'s flags; the `REQUIRES` of each method rule out settings that
contradict each other, such as `[glob, fixed text]`.

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
echo 'find "lib" with [(file) only]' | anyx-fd --anyx-protocol
echo 'SCHEMA' | anyx-fd --anyx-protocol
```
