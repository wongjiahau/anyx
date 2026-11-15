---
title: A value with no type of its own is always annotated with AS:, even without overloading
author: Wongjiahau
date: 2026-11-15
branch: work/overloading
id: l-19a5c0de
scope: repo
status: active
src: agent
touches:
  - crates/anyx-protocol/PROTOCOL.md
  - crates/anyx-protocol/src/annotation.rs
  - crates/anyx-protocol/src/codec.rs
  - crates/anyx-protocol/src/check.rs
  - crates/anyx-protocol/src/json.rs
  - crates/anyx-protocol/src/guide.rs
  - crates/anyx-protocol-generate/src/lib.rs
  - crates/anyx-protocol-generate/README.md
---

## Decided
- Methods may be overloaded: the same shape in a scope with different parameter types, resolved per call by the arguments' types (issue #19).
- In a request, an argument must have its parameter's type, of its own or by an annotation, even when the method has no overloads, so adding an overload later never breaks an existing call. Examples: `tag ([] AS: LIST: TEXT)`, `open ("abc" AS: file id)`. Without it, the whole request is `INVALID REQUEST` up front, and the message shows the fix: "in `id` of `read _`: `"abc"` has the type TEXT of its own, not `file id`; annotate it, as in `read ("abc" AS: file id)`".
- A value's own type comes from how it is written and the schema's types, never its methods (`annotation::own_type`): a number is INTEGER or FLOAT by its form (#22); a string is TEXT; `true`/`false` is BOOLEAN, even next to a choice with a variant `true`; numbers with units are DURATION; a non-empty list or string-keyed map of items of one own type is `LIST`/`MAP` of it; braces of fields are the one non-generic record of the whole schema with each written field and none of its required ones missing; a variant is the one non-generic choice of the whole schema with a variant of that shape. Nothing else has one: `[]`, `{}`, generic types, records or variants several types fit. So BYTES, INSTANT and named types are always annotated: they are written like a string or the type underneath.
- Only a whole argument is annotated. Values inside an argument are read by the type around them as before, and `AS` inside a value is an error.
- The annotated type is read with the schema grammar's `type` rule (`grammar::decode_type`) and resolved in the method's scope: built-ins, the scope's types, or a full path to a `PUBLIC` one. It must equal the parameter's type exactly, without reading past named types. It is a claim, not a conversion: the value is then decoded against it.
- Resolution: of the methods of the call's shape in its scope, the one whose parameter types equal the arguments' stated types (annotation, else own). One method: decode against it, then reject an argument that states another type or none (decoding first keeps the old, more specific messages for a wrong value). Several: the one that fits, else an error listing the typed shapes, with a fix when an argument has no type of its own.
- Schema check (`check.rs`): two methods of a shape in a scope whose parameter types pairwise overlap (`annotation::overlaps`: equal, with a type parameter overlapping anything) are an error; a hint is added when they differ in what they return. This replaces "a second method".
- `EXAMPLES` are calls, so they follow the request rules (`decode_call_of` takes the schema file's source for the fix). `DEFAULT`s are read by the field's type.
- `encode_request` writes each argument, parses it back, and annotates it with its parameter's type when its own type differs, so the Rust client and the TypeScript client (through wasm) annotate by themselves. The formatter drops the `:` where nothing would join, so a request reads `open ("a" AS file id)` and `tag ([] AS: LIST: TEXT)`; both spellings read the same.
- JSON names an overload by its typed name, its full name with its parameters' types in the slots as written in its scope (`Schema::typed_name`/`call_name`): `scale (FLOAT)`, `tag (LIST: file id)`. That is how JSON chooses between INTEGER and FLOAT (or TEXT and a named type), which its values can't show. Any method may be named that way; the plain name of an overloaded one is an error listing the typed names. `decode_request_json`, the generated TypeScript calls and `EFFECTS` keys use `call_name`.
- Generated names: an overload is its shape's words + `with` + each parameter's type in words joined by `and` (`LIST: T` is `list of T`, a schema type is its scope's words then its own, a generic one has its arguments' words in its slots): `tag_with_list_of_text`, `TagWithListOfTextError`, `SCALE_WITH_FLOAT_EFFECTS`. Methods without overloads keep their names. A clash goes through the existing `distinct` check.
- The guide explains annotations in Values, lists each shape's overloads together in Requests by typed shape, annotates its made-up sample calls, and a scope's "Methods:" line names overloads by typed name, together.

## Tried and ruled out
- `("abc": file id)` and `(file id: "abc")`: in a value position, `a: b` already means `a (b)`, and a value may be any anyx expression. `:` is a name annotation only in a parameter, where the part before it is always a name.
- Accepting `open "abc"` for a named type, and rejecting only calls that several overloads fit: adding an overload that takes the underlying type would break those calls later.
- A record's or variant's own type taken from the parameters it could fill: then adding an overload with another choice that shares a variant would break existing calls.
- Generated overload names from parameter names: they are often the same across overloads (`id`), and types are what tell overloads apart.

## Watch out
- A record's or variant's own type is unique across the whole schema, so adding a record whose fields also fit, or a choice with a variant of the same shape, makes such arguments need annotations. The checker doesn't warn about it.
- Adding a second method of a shape renames the first in generated code (`tag` becomes `tag_with_list_of_text`), and its JSON/TypeScript name becomes its typed name.
- A wrong value is reported before a missing annotation: `read 3` for a `file id` says "expected a string", not "annotate it".

In the code: `own_type`, `annotation`, `annotated_type` and `overlaps` in crates/anyx-protocol/src/annotation.rs; `decode_call`, `decode_arguments`, `choose`, `fix` and `write_call` in src/codec.rs; `is_overloaded`, `typed_name`, `call_name` and `type_name_in` in src/schema.rs; `method_index` in src/json.rs; `method_words` in crates/anyx-protocol-generate/src/lib.rs. Tests: crates/anyx-protocol/tests/overloading.rs, crates/anyx-protocol-generate/tests/overloaded_round_trip.rs and the `overloaded` fixture and snapshots.
