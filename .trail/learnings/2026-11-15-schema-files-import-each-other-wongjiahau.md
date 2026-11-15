---
title: Schema files import each other explicitly with paths relative to the importing file, and the generator starts from an entry file
author: Wongjiahau
date: 2026-11-15
branch: master
id: l-3a6f0c58
scope: repo
status: active
src: agent
touches:
  - crates/anyx-protocol/PROTOCOL.md
  - crates/anyx-protocol/src/schema.rs
  - crates/anyx-protocol-generate/src/bin/anyx-protocol-generate.rs
---

## Decided
- No implicit imports. `USE name FROM "relative path"` mounts a file under a name the importer chooses. Each file imports what it uses and refers to the imported file's `PUBLIC` declarations as `name/Thing` (#27).
- The mount name is the API, not the file name. Call paths are built from mount names starting at the entry file, so renaming a file doesn't change the wire.
- A library is just a file without a `PROTOCOL` header. There is no `LIBRARY` keyword. A file is loaded once, however many files import it.
- The generator takes an entry file, the one with the `PROTOCOL` header, and follows `USE` lines. Folder input and the special `protocol.anyx` name go away.

## Tried and ruled out
- Folder-based scopes (every `.anyx` file under the root is a scope named by its path, the current behaviour): moving or renaming a file changes the API, a stray file joins the protocol, and a file can't be read on its own.
- Root-relative paths (`/common.anyx`): they need a marker to find the root, which is implicit again. A shared library would need its own root, which means designing packages too early. Subtrees can't be copied. Checking can't start from any file.
- "Relative imports are harder to maintain": with agents and LSP rename refactors, rewriting `../` is cheap, and a broken path fails loudly. A named package root can come later.

In the code:
- Today's folder loading: `read_schema` and `collect` in `crates/anyx-protocol-generate/src/bin/anyx-protocol-generate.rs`, `ROOT_FILE` and `Schema::parse_files` in `crates/anyx-protocol/src/schema.rs`.
