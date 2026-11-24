# anyx-protocol-generate

Writes the code of an [anyx-protocol](../anyx-protocol) schema:

```sh
anyx-protocol-generate rust schema.protocol.anyx > src/protocol.rs
anyx-protocol-generate wasm schema.protocol.anyx > src/protocol_wasm.rs
anyx-protocol-generate typescript schema.protocol.anyx > web/src/protocol.ts
```

Pass an entry file with a `PROTOCOL` header. Its `USE "relative path"`
imports are followed explicitly; folder input is rejected. See
[`PROTOCOL.md`](../anyx-protocol/PROTOCOL.md#files-and-use).

Input filenames, including every explicitly imported file, must end in
`.protocol.anyx` (case-sensitive). Other suffixes are rejected before reading
the file.

## Rust

- A struct per record and an enum per alias of a union of variants,
  `` TYPE ALIAS Colour [| `dark, `custom hue (hue ⁽Float) |] ``, with a
  variant per shape:
  `custom hue (hue ⁽Float)` is `CustomHue { hue: f64 }`.
- `Instant` is `std::time::SystemTime` and `Duration` is
  `std::time::Duration`; the wire keeps both to the millisecond.
- `MAP ⁽T` is `std::collections::BTreeMap<String, T>`, and
  `Record<string, T>` in TypeScript.
- A field with a `DEFAULT` may be omitted on input. The reader fills in
  its default, so generated records always contain the field.
- A single-slot `NAMED TYPE File Id (value ⁽Text)` generates
  `pub struct FileId(pub String)` with `From` conversions both ways.
  A multi-slot `NAMED TYPE Span from (start ⁽Integer) to (end ⁽Integer)`
  generates `pub struct SpanFromTo { pub start: i64, pub end: i64 }`.
  TypeScript generates a branded type and a constructor function, such as
  `SpanFromTo({ start: 1n, end: 3n })`.
- Multi-slot record fields use a struct in Rust and an object in TypeScript.
  `quiet from (start ⁽Integer) to (end ⁽Integer)` is accessed as `quiet.start`.
- A `NAMED TYPE` whose slot has a `WHERE` refinement has private fields, a
  checked `new` constructor, and `get` / `into_inner` accessors and
  `TryFrom` for a single slot. Its `FromValue` conversion checks the
  predicate too.
- A `TYPE ALIAS` is only another name for its type, so the generated code
  uses that type. Refinements on it and inline refinements use the base
  Rust type and are checked by the codec on input and output.
- A generic record or choice is a generic struct or enum. Its name is the
  PascalCase of its words, leaving out the slots, and it has a type
  parameter per slot, in PascalCase and in the slots' order, with its
  `IntoValue` and `FromValue` bound on them: `TYPE ALIAS Page of (Item ⁽TYPE) ⁽{ items (items ⁽LIST Item) }`
  is `pub struct PageOf<Item>`, with
  `impl<Item: IntoValue> IntoValue for PageOf<Item>`, and `page of (order)`
  is `PageOf<Order>`. `TYPE ALIAS (Item ⁽TYPE) history ⁽{ items (items ⁽LIST Item) }` is `History<Item>`,
  `TYPE ALIAS Table from (Key ⁽TYPE) to (Value ⁽TYPE) ⁽{ keys (LIST Key), values (LIST Value) }` is
  `TableFromTo<Key, Value>`, and `TYPE ALIAS (Left ⁽TYPE) or (Right ⁽TYPE) [| … |]` is
  `Or<Left, Right>`. Two types whose words are the same, such as `page` and
  `page (Item ⁽TYPE)`, would get the same name, so generating either
  language is an error that names both. A parameter whose name the
  module already uses, such as `value` (the generated code uses `Value`) or
  a type of the protocol, gets `Parameter` after it: `ValueParameter`. The
  same goes in TypeScript, `interface PageOf<Item>`, against every type there.
- An enum of each method's errors, named after the method:
  `write _ at revision _ content _` has `WriteAtRevisionContentError`. It
  holds every error flat: the variants in its `ERRORS` brackets and those
  of each alias named there. For each such alias, say `File Errors`, it
  has `impl From<FileErrors>`, so `?` turns a `FileErrors` into it.
- A `Server` trait with an `async fn` per method. A method with errors
  returns `Result<Result<T, MethodError>, Self::Error>`; the outer `Err` is
  the server's own failure, which the caller sees as `INTERNAL ERROR`. A
  method that `RETURNS NOTHING` returns `()`, and `void` in TypeScript.
  Each method's doc comment, in Rust and TypeScript, says what its effect
  markers claim (`READ ONLY`, `IDEMPOTENT`, neither, and `UNDONE BY`), then
  ends with its `EXAMPLES`, each after its `#` line, in an `anyx` code block.
- A constant of each method's markers, `READ_EFFECTS` for `read _`, an
  `anyx_protocol::client::Effects` with `read_only`, `idempotent` and
  `undone_by`, so a tool can decide whether to retry a call or ask before
  sending it. Only an idempotent call (`may_retry()`) may be retried on its
  own. TypeScript has `EFFECTS`, the same by the method's full name:
  `EFFECTS["read _"].readOnly`. Neither client retries anything itself.
- `handle(server, request, on_internal_error)`, which reads a request, runs
  its calls and writes the response. It answers `SCHEMA` itself.
- A `Client<T: Transport>` with a method per schema method, and `schema()`,
  which calls the built-in `SCHEMA` and returns the guide.
- `SCHEMA_FILES`, the schema as written, and `schema()`, the schema read
  once, for `anyx_protocol::guide`.

All declarations are generated into one Rust module, with one `Server`
trait and a flat client:

```rust
impl poentaro::Server for Agent {
    type Error = worker::Error;
    async fn read(&self, path: String) -> Result<Result<File, ReadError>, Self::Error> { … }
    // Implement the remaining methods here.
}
let pdf = client.pdf(path).await;
```

The module needs `anyx-protocol`.

## TypeScript

The `typescript` output is a standalone ES module with a schema-specific
native codec. It needs no wasm or runtime validation package, and runs in
Node, browsers, and edge runtimes with ES2022 support. Supply a transport
that sends and receives AnyX text:

```ts
const client = new Client(
  (request) => fetch(url, { method: "POST", body: request }).then((r) => r.text()),
);
const outcome = await client.read("hymn.poentaro");
const value = unwrapOutcome(outcome);
const [a, b] = await client.batch((calls) => [calls.read("a"), calls.read("b")]);
```

`Integer` is a `bigint` in the inclusive range -2^63 through 2^63-1; use
`BigInt` to convert safe integers; before converting back to `number`,
check that the bigint is between `BigInt(Number.MIN_SAFE_INTEGER)` and
`BigInt(Number.MAX_SAFE_INTEGER)`. `Bytes` is `Uint8Array` and is encoded on the
wire as padded standard base64. `Float` is a finite JavaScript `number`.
`Instant` is `Date`; `Duration` is a `bigint` number of milliseconds
from 0 through 2^64-1. Lists are arrays, maps are string-keyed objects, records use
camelCase fields, and choices and declared errors use a `kind` discriminant.
Named `TYPE`s are branded and have a same-named constructor. A refined
constructor validates its value and throws on failure. The native codec
checks refinements on requests and responses too, including nested fields,
container items and generic arguments. Integer predicates use bigint
arithmetic without rounding; text length counts Unicode scalar values.

Every response is parsed and checked against the expected method before an
outcome is returned. Invalid text, incorrect values, undeclared errors and
incorrect batch counts reject the promise, as do transport or request encoding
failures. A batch rejects all queued calls if its transport or codec fails.
An `invalidRequest` response is returned for each call in a batch. Declared
errors, `internalError`, `notRun`, and `invalidRequest` remain outcome
variants. `unwrapOutcome` returns a value or throws `OutcomeFailure`, whose
`outcome` property preserves the declared error or protocol failure.

The optional `wasm` output remains available separately for applications
that use its JSON interface.

TypeScript uses plain names: `Diagnostic` and `ReadError`. Shared library
types are copied into each protocol's generated output so outputs stand alone.

Overloads, methods of one shape with different parameter types, each get a
name of their own: the shape's words, `with`, then each parameter's type in
words, joined by `and`. `LIST ⁽T` is `list of T`, `MAP ⁽T` is `map of T`, a
type of the schema is its plain words, and a generic type has
its arguments' words in its slots. An overload of `open` taking a
`File Id` becomes `open_with_file_id`, and one of `scale` taking a
`Float` becomes `scale_with_float` (`scaleWithFloat` in TypeScript). A method without overloads keeps its shape's words, so
adding an overload renames the methods of its shape. The server runs the
overload a call resolves to, and the clients write the annotations a
request needs themselves. TypeScript calls an overload, and keys its
`EFFECTS`, by its typed name, `"scale (Float)"`, as JSON does.

A schema whose names become the same name in the generated code, such as a
type `read error` next to the method `read _`, or a method `tag with text`
next to the overload `tag (Text)`, is rejected.

The prelude's `Boolean` and postfix `T?` are ordinary choices. They generate
`Boolean` and `Nullable<Item>` enums in Rust and tagged unions in TypeScript.
`DEFAULT` makes a record field omittable on input; decoded records always
contain that field. The protocol has no `OPTIONAL` modifier.
