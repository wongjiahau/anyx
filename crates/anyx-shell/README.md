# anyx-shell

An interpreter for scoped anyx protocols. Each tool has its
own schema and session state. Successful commands return anyx values:
records, lists, strings, numbers or choices. The shell displays these values
as anyx, with no `RETURNS` envelope or text-table rendering.

## Run a document

```sh
cargo run -p anyx-shell -- crates/anyx-shell/examples/environment.shell.anyx
cargo run -p anyx-shell -- --expression 'environment / list variables'
printf 'environment / read variable "HOME"' | cargo run -p anyx-shell
```

With no arguments, start an interactive prompt when standard input and output
are terminals; otherwise read standard input as a batch document. `-` always
reads a batch document. With one file argument, read that file.
`--expression SOURCE` runs an inline document. `--help` returns usage as an anyx text value. Saved shell documents use `.shell.anyx`.

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

## Interactive shell

```sh
cargo run -p anyx-shell
# Or explicitly select interactive mode:
cargo run -p anyx-shell -- --interactive
```

```text
anyx> environment / set variable "EDITOR" to "helix"
{ name "EDITOR", value "helix" }
anyx> environment / read variable "EDITOR"
"helix"
```

Each submitted document runs in the same interpreter. Errors are displayed and
the prompt returns; successful earlier changes remain available. Ctrl-C cancels
the current input and Ctrl-D at an empty prompt exits successfully. Input/output
failures still exit with an error. Up/Down recall commands from this session;
the last 1,000 history entries stay in memory and are never saved to disk.

Completion opens while typing and shows matching scopes or full method
signatures, parameter types, return types, and protocol documentation. Use
Up/Down to choose an entry, Enter or Tab to accept it, and Esc to dismiss it.
Ctrl-Space opens completion explicitly. Accepting a menu entry edits the input;
it never runs a command.

Methods insert editable argument templates. For example, type `env`, press Tab,
type `set`, and press Enter:

```text
anyx> environment / set variable "name" to "value"
                                ^^^^ selected
```

Type a name to replace the selection, then Tab to select `value`. Shift-Tab goes
back to the previous argument, including its current edited value. Tab after
the last argument leaves snippet editing and moves to the end of the template.
Esc also leaves snippet editing. The selected argument is highlighted, with its
name, type, and position shown below the input. Press Enter with no menu open
to run the command. Template values are editable defaults, so replace them
before submitting.

Completion reads registered tools' schemas without executing them. It works
inside nested calls and after commas or grouping operators, and preserves
existing arguments when editing a method word in the middle of a command.
Record-field completion, choice-variant menus, and filtering nested calls by
expected return type are future work.

The REPL uses Ratatui in an inline viewport, keeping command output in terminal
scrollback. Long input lines scroll horizontally to keep the cursor visible.

Left/Right move by Unicode graphemes. Up/Down move through multiline input or
history when the completion menu is closed. Home/End and Ctrl-A/Ctrl-E move to
line boundaries; Backspace/Delete edit text, and Ctrl-W/Ctrl-U/Ctrl-K delete a
word, the prefix, or the suffix of the current line. Bracketed paste inserts
text without submitting it. Alt-Enter inserts a newline explicitly.

Open brackets, unfinished quoted/raw strings, and a trailing `:` or `.` continue
input onto another line. Other invalid input is submitted for a diagnostic.
Newlines alone do not separate commands: use commas to submit several calls
in one document. Wrap a multiline argument in brackets or parentheses to keep
input open while editing.

## Scopes and protocols

Every call must name a registered tool: `environment / list variables` or
`environment/list variables`. Unqualified `list variables` is rejected.
Scopes belong to the shell, so protocols retain local method names and a
flat namespace. The same method name can occur in several tools without a
collision. The built-in protocol discovery call is scoped too:

```anyx
environment / SCHEMA
```

It returns the environment protocol's guide as an anyx text value. The shell
accepts protocol literals and evaluated argument expressions. Protocol wire
requests still carry values; evaluation belongs to the shell.

## Argument expressions and types

A method argument can be an expression, including another scoped call:

```anyx
environment / set variable "COPY" to (environment / read variable "HOME")
```

Only the outer call's variable record is displayed. The nested text value
becomes its argument. Expressions also work inside lists, record fields, map
values, and choice arguments. Anyx's grouping sugar works too:

```anyx
environment / set variable "COPY" to: environment / read variable "HOME"
```

The checker synthesizes argument types from the inside out, using each
nested method's declared return type. Those argument types select the enclosing
overload; its expected result type never selects its own overload. All calls
are checked before any tool runs, including calls nested in containers.
Unambiguous literals infer their type as in protocol requests. Empty lists,
ambiguous variants, and named types can use `AS: Type` annotations. A computed
value keeps its declared type even when its runtime value is an empty list.
An annotation must agree with a computed expression's type; it is not a cast.
Record fields and choice slots check their contents against their declared types.

Built-in types cross tool boundaries directly. Protocol-local named types keep
their identity: another tool must declare the same name and reachable type
definitions to consume one. Unrelated types cannot mix just because their
values have the same representation. Methods returning nothing cannot appear
as arguments.

At runtime, arguments evaluate from left to right, before their enclosing
method. A nested failure stops remaining arguments and the enclosing call.
Earlier successful effects remain. Lists, records, and maps evaluate their
contents in source order. Unknown expressions produce a diagnostic; accepting
anyx syntax does not invent arithmetic operators or unregistered functions.

Run the example with:

```sh
cargo run -p anyx-shell -- crates/anyx-shell/examples/expressions.shell.anyx
```

Shell variables, pipelines and subprocesses are future work.

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
In batch mode, every failure exits with a nonzero status. Interactive mode
reports command errors and keeps running. Batch output contains no human-text
preambles or debug representations.

## Verify

```sh
cargo test -p anyx-shell
# Terminal integration checks on Unix:
cargo build -p anyx-shell
python3 crates/anyx-shell/tests/repl_terminal.py
```
