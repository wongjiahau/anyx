# anyx-protocol

An RPC protocol written in anyx. A schema declares the methods; requests and
responses are anyx documents, which people and AI agents read and write
easily, checked against the schema.

```rust
let schema = anyx_protocol::Schema::parse(source)?;
// Or follow explicit imports through a reader:
// Schema::load("notes/protocol.protocol.anyx", |path| std::fs::read_to_string(path))
let calls = anyx_protocol::decode_request(&schema, request)?;
// Run each call, then:
let response = anyx_protocol::encode_response(&schema, &calls, &outcomes)?;
```

`guide(schema, how_to_send)` is the markdown to hand an agent first. See
[`PROTOCOL.md`](PROTOCOL.md) for the schema language and the wire format.

Rather than calling the codec by hand, generate a typed server and client
with [`anyx-protocol-generate`](../anyx-protocol-generate).

`encode_value(schema, expected_type, value)` writes a checked standalone anyx
value without a response envelope, for consumers such as `anyx-shell`.

`decode_argument` reads a literal argument and synthesizes its type,
`decode_value` reads a literal in a known type, and `decode_type` resolves a
type expression. These let consumers type-check expressions without changing
the protocol's literal-only wire requests.
