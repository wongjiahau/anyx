---
title: Built-in types are declared in a prelude written in anyx, marked INTRINSIC, and type names are written in Title Case
author: Wongjiahau
date: 2026-11-15
branch: master
id: l-7e2d91b4
scope: repo
status: active
src: agent
touches:
  - crates/anyx-protocol/protocol.anyx
  - crates/anyx-protocol/src/time.rs
---

## Decided
- Every built-in type is declared in a prelude, `crates/anyx-protocol/prelude.anyx`. It is written in the schema language, with `#` docs, and loaded before every schema (#26).
- `INTRINSIC` can mark any kind of declaration. It means the shape is written in anyx and the implementation supplies the meaning. Duration is one declaration: `INTRINSIC CHOICE Duration [day, hour, minute, second, millisecond]`. The implementation supplies that a value is counts followed by units, largest first, that singular and plural both read, and that the total is in milliseconds. Only the prelude may use `INTRINSIC`.
- Type names are written in Title Case (`Duration`, `File Summary`). This is a naming convention, not enforced: `Duration` is an ordinary non-keyword word in anyx. The built-ins are renamed to match (`INTEGER` → `Integer`). Fields, parameters, method words and variants stay lowercase. All caps stays for syntax.

## Tried and ruled out
- All-caps built-in names (`CHOICE TIME UNIT`): all caps marks syntax in anyx, not names. It also needed `LIST: TIME UNIT`.
- A separate `time unit` choice beside an intrinsic `DURATION`: one `INTRINSIC CHOICE Duration` says the same with one declaration.
- All-lowercase built-ins (`duration`): types would look like fields and values. Title Case tells them apart.
- Growing the schema language (repeated parts) so a duration's full shape can be declared: that adds a feature few schemas would use. `INTRINSIC` covers it.
- Having the checker enforce the casing: the user wants it to stay a convention, like any non-keyword word.

## Watch out
- A word with no lowercase letters is a keyword (`is_keyword` in `crates/anyx/src/lexer.rs`). Write `Id` and `Http Request`, never `ID` and `HTTP Request`.

In the code:
- Units table to replace: `UNITS` and `parse_duration` in `crates/anyx-protocol/src/time.rs`. Issues #26 (prelude) and #25 (request grammar).
