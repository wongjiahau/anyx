---
title: "Responses answer each call by position, without repeating the call"
author: Wongjiahau
date: 2026-11-14
branch: master
cost: { minutes: 30, tokens: 121102 }
id: l-377539f4
scope: repo
status: active
src: agent
touches:
  - crates/anyx-protocol-generate/README.md
  - crates/anyx-protocol-generate/src/lib.rs
  - crates/anyx-protocol-generate/src/rust.rs
  - crates/anyx-protocol-generate/src/typescript.rs
  - crates/anyx-protocol-generate/tests/fixtures/generic.anyx
  - crates/anyx-protocol-generate/tests/generated.rs
  - crates/anyx-protocol-generate/tests/generated/generic.rs
  - crates/anyx-protocol-generate/tests/generated/generic.ts
  - crates/anyx-protocol-generate/tests/generic_round_trip.rs
  - crates/anyx-protocol/PROTOCOL.md
---

## Decided
- A response is one result per call, in the order of the calls, separated by commas: `RETURNS value` (or `RETURNS NOTHING`), `ERROR error`, `INTERNAL ERROR`, `NOT RUN`. A result no longer repeats the call it answers; a reader matches results to calls by position. `INVALID REQUEST { line, column, message }` is unchanged: one result in place of all of them.
- Why: the repeated call echoed arguments, secrets included. `account/change password from "old" to "new" RETURNS NOTHING` carried both passwords back, and responses end up in HAR files, caches, devtools and agent transcripts. PROTOCOL.md already said readers check the method and don't read the arguments, so the echo bought little.
- Decoding checks the number of results equals the number of calls ("found 1 result; expected 2 results, one per call, in the order of the calls", pointing at the first extra result), and reads each result as an outcome of the method of the call in its place. With several calls, a mistake is prefixed "in result 2 of 3, for the call of `read _`". A result that still starts with a call (`read "a" RETURNS …`) gets the hint "a result doesn't repeat its call, so write it alone".
- `EXAMPLES` keep the call and its outcome side by side (`write "a" at revision 3 content "x" RETURNS 4`), since an example shows a request and its response together; only the wording changed. The checker and the response reader share `WrittenOutcome` and `read_outcome`.
- Everything that existed only for the repeated call is gone: `Form`/`Form::Shortened`, `SHORTENED_CHARACTERS` (32) and `SHORTENED_ITEMS` (3), the label check `repeated_method`, the `Split` of a result into call and ending, and their messages. JSON outcomes were already positional and needed nothing.

## Tried and ruled out
- Repeating the full call with long arguments shortened (the format since commit 97d5bbc): leaked secrets; short ones were never shortened.
- Repeating only the method name (`change password from _ to _ RETURNS NOTHING`): rejected by the user in favour of fully positional results.
- A `SECRET` type that redacts its value in the echo: dropped for now, since without the echo its main reason is gone. It may come back for logging.

## Watch out
- Language models can miscount a long list of results. The guide says so and asks the reader to keep a request short enough to count its results; keep that advice if the guide is rewritten.
- A response is only readable next to its request now: keep the request (or the list of methods) around wherever a response is stored or decoded.

In the code: crates/anyx-protocol/src/response.rs (`encode_outcome`, `decode_response`, `written_outcome`, `repeats_a_call`, `read_outcome`); `write_text` and `write_quoted` in src/codec.rs; `Checker::examples` in src/check.rs; the Responses section of src/guide.rs and crates/anyx-protocol/PROTOCOL.md; tests in crates/anyx-protocol/tests/codec.rs and tests/snapshots/positional_response_errors.txt.
