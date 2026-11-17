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

- A struct per `RECORD` and an enum per `CHOICE`, with a variant per shape:
  `custom hue (hue: Float)` is `CustomHue { hue: f64 }`.
- `Instant` is `std::time::SystemTime` and `Duration` is
  `std::time::Duration`; the wire keeps both to the millisecond.
- `MAP: T` is `std::collections::BTreeMap<String, T>`, and
  `Record<string, T>` in TypeScript.
- An `OPTIONAL` field is an `Option`, and `?` in TypeScript, unless it has a
  `DEFAULT`: the reader fills that in, so the field is always there.
- A struct around the type underneath per named `TYPE`, with `From` both
  ways: `TYPE File Id: Text` is `pub struct FileId(pub String)`, so a
  `FileId` can't be passed where a `ShareId` goes.
- A generic record or choice is a generic struct or enum. Its name is the
  PascalCase of its words, leaving out the slots, and it has a type
  parameter per slot, in PascalCase and in the slots' order, with its
  `IntoValue` and `FromValue` bound on them: `RECORD Page of (item: TYPE)`
  is `pub struct PageOf<Item>`, with
  `impl<Item: IntoValue> IntoValue for PageOf<Item>`, and `page of (order)`
  is `PageOf<Order>`. `RECORD (item: TYPE) history` is `History<Item>`,
  `RECORD Table from (key: TYPE) to (value: TYPE)` is
  `TableFromTo<Key, Value>`, and `CHOICE (left: TYPE) or (right: TYPE)` is
  `Or<Left, Right>`. Two types whose words are the same, such as `page` and
  `page (item: TYPE)`, would get the same name, so generating either
  language is an error that names both. A parameter whose name the
  module already uses, such as `value` (the generated code uses `Value`) or
  a type of the protocol, gets `Parameter` after it: `ValueParameter`. The
  same goes in TypeScript, `interface PageOf<Item>`, against every type there.
- An enum of each method's errors, named after the method:
  `write _ at revision _ content _` has `WriteAtRevisionContentError`.
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

## wasm and TypeScript

TypeScript only sends the request: encoding and decoding stay in Rust. The
`wasm` output has two `#[wasm_bindgen]` functions, such as
`encodePoentaroRequest` and `decodePoentaroResponse`, for a crate that has
`wasm-bindgen` and `anyx-protocol` with its `json` feature. The `typescript`
output has the types as JSON (camelCase names, and a `kind` on each variant
and error) and a `Client`:

```ts
const client = new Client(
  { encodeRequest: encodePoentaroRequest, decodeResponse: decodePoentaroResponse },
  (request) => fetch(url, { method: "POST", body: request }).then((r) => r.text()),
);
const outcome = await client.read("hymn.poentaro");
const pdf = await client.pdf("hymn.poentaro");
// Several calls in one request, run in order:
const [a, b] = await client.batch((calls) => [calls.read("a"), calls.read("b")]);
```

The wasm functions take and give every call of a request at once:
`encodePoentaroRequest('[{"method":"read _","arguments":["a"]}]')` and
`decodePoentaroResponse('["read _"]', response)`, one outcome per call, in
the order of the calls.

An `Instant` is a `Date`: the client turns the text JSON carries into one,
and `JSON.stringify` writes a `Date` back as that text. A `Duration` is a
`number` of whole milliseconds.

A named `TYPE` is a branded type, with a function of the same name that
brands a value: `TYPE File Id: Text` is
`type FileId = string & { readonly __brand: "file id" }` and
`FileId("a")`. Its JSON is the value underneath.

A generic type's reviver takes the reviver of each parameter and returns
its own, and a use passes them: a method returning
`page of (Payment)`, where a payment holds an `Instant`, revives with
`revivePageOf(revivePayment)`, and one returning `page of (Text)` needs
no reviving, unless `page of (item: TYPE)` holds an `Instant` itself, when it passes
`unchanged`.

TypeScript uses plain names: `Diagnostic` and `ReadError`. Shared library
types are copied into each protocol's generated output so outputs stand alone.

Overloads, methods of one shape with different parameter types, each get a
name of their own: the shape's words, `with`, then each parameter's type in
words, joined by `and`. `LIST: T` is `list of T`, `MAP: T` is `map of T`, a
type of the schema is its plain words, and a generic type has
its arguments' words in its slots: `tag (paths: LIST Text)` is
`tag_with_list_of_text` (`tagWithListOfText`, `TagWithListOfTextError`,
`TAG_WITH_LIST_OF_TEXT_EFFECTS`), `open (id: File Id)` is
`open_with_file_id`, and `scale (factor: Float)` is
`scale_with_float`. A method without overloads keeps its shape's words, so
adding an overload renames the methods of its shape. The server runs the
overload a call resolves to, and the clients write the annotations a
request needs themselves. TypeScript calls an overload, and keys its
`EFFECTS`, by its typed name, `"tag (LIST Text)"`, as JSON does.

A schema whose names become the same name in the generated code, such as a
type `read error` next to the method `read _`, or a method `tag with text`
next to the overload `tag (Text)`, is rejected.
