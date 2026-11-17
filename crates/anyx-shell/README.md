# anyx-shell

A non-interactive interpreter for scoped anyx protocols. Each tool has its
own schema and session state. Successful commands return anyx values:
records, lists, strings, numbers or choices. The shell displays these values
as anyx, with no `RETURNS` envelope or text-table rendering.

## Run a document

```sh
cargo run -p anyx-shell -- crates/anyx-shell/examples/environment.shell.anyx
cargo run -p anyx-shell -- --expression 'environment / list variables'
printf 'environment / read variable "HOME"' | cargo run -p anyx-shell
```

With no arguments or `-`, read standard input. With one file argument, read
that file. `--expression SOURCE` runs an inline document. `--help` returns
usage as an anyx text value. Saved shell documents use `.shell.anyx`.

Calls are comma-separated; newlines are whitespace:

```anyx
environment / set variable "EDITOR" to "helix",
environment / read variable "EDITOR",
environment / remove variable "EDITOR",
environment / remove variable "EDITOR",
```

Standard output:

```anyx
{ name "EDITOR", value "helix" },
"helix",
true,
false,
```

An empty collection is a list `[]`; an empty document or a
method that returns nothing produces no output. Multiple results form one
comma-separated anyx document.

## Scopes and protocols

Every call must name a registered tool: `environment / list variables` or
`environment/list variables`. Unqualified `list variables` is rejected.
Scopes belong to the shell, so protocols retain local method names and a
flat namespace. The same method name can occur in several tools without a
collision. The built-in protocol discovery call is scoped too:

```anyx
environment / SCHEMA
```

It returns the environment protocol's guide as an anyx text value. Calls
use the protocol's existing request syntax and argument validation. This
first interpreter runs literal calls; variables, expressions that feed
results to later calls, pipelines, subprocesses and an interactive prompt
are future work. Top-level `.` and `:` chaining are rejected.

The library exposes `Interpreter` and a synchronous `Tool` trait. Register a
protocol implementation under a lowercase scope with
`interpreter.register("environment", Environment::new())`; the interpreter
checks requests against `Tool::schema()` before calling `Tool::invoke()`.
Schemas must remain stable. Returned values and declared errors are checked
against that same schema. Additional tools need no changes to routing.

## Environment example

[`environment.protocol.anyx`](environment.protocol.anyx) defines four methods:

| Method | Returned value |
| --- | --- |
| `list variables` | A list of `{ name, value }` records, sorted by name |
| `read variable (name)` | A text value, or an error if absent |
| `set variable (name) to (value)` | The resulting `{ name, value }` record |
| `remove variable (name)` | Whether a variable was removed |

The executable snapshots its inherited environment into a session-owned map.
Changes affect that session, never the parent shell or Rust's process-global
environment. Embedders can reuse an `Interpreter` across documents; a new
executable invocation starts a new session. Names are case-sensitive on all
platforms. Empty values are present values. Names cannot be empty or contain
`=` or a null character; values cannot contain a null character. Inherited
non-Unicode names or values cause a diagnostic instead of lossy conversion.

## Failures

The entire document is parsed and schema-checked before any command runs.
A malformed call, unknown scope, unknown method or incorrectly typed argument
means none of its calls run. Empty and comment-only documents succeed.

Execution proceeds in order and stops on the first declared method error
or internal failure. Earlier successful effects and results remain; a batch
is not a transaction. Standard output contains earlier returned values, and
standard error contains an anyx error expression, such as:

```anyx
ERROR variable does not exist
```

Input and validation failures use `INVALID REQUEST { line, column, message }`,
with one-based source positions. Internal failures use `INTERNAL ERROR`.
Every failure exits with a nonzero status. The CLI prints no human-text
preambles or debug representations.

## Verify

```sh
cargo test -p anyx-shell
```
