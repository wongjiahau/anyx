---
title: A protocol is one flat namespace split across files that import each other with USE and a relative path
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
- A protocol is one flat namespace. Folders and files are purely cosmetic and informational (#27).
- `USE "relative path"` is the only import form, with no names in it. It makes the file part of the protocol, and makes the file's `PUBLIC` declarations visible here, unqualified. There is no re-export.
- Every type and error-group name is declared once in the whole protocol, private ones included. Method shapes are unique except for valid overloads. There are no call paths (`files/export/pdf` becomes `pdf`), and annotations use plain names (`AS File Id`).
- Paths are relative to the importing file, and `..` is allowed. A file without a `PROTOCOL` header is a library, and its methods, if any, join every protocol that uses it.
- Tools take an entry file (the one with the header) as an argument and follow `USE`. Folder input and `ROOT_FILE` go away.
- In the anyx shell, each program is the namespace (`notes/pdf`). That is the only qualification, and the program name the user picks is the only alias.

## Tried and ruled out
- Folder-based scopes (the current behaviour): moving a file changes the API, stray files join the protocol, and the layout leaks into requests (`AS common/File Id`).
- Root-relative paths (`/common.anyx`): they need a root marker (implicit again) and per-library roots (packages too early), subtrees can't be copied, and checking can't start from any file. Relative paths' maintenance cost is cheap with agents and LSP refactors.
- Per-file namespaces (`USE common FROM …`, then `common/Diagnostic`): adds aliases and clash rules for nothing. GraphQL shows that one flat type namespace scales. The shell's per-program prefix covers names from different authors.
- `MOUNT` beside `USE`, to separate "methods callable at a path" from "names imported": with no call paths, a method can't be reachable twice, so one keyword is enough. The entry file is chosen by the tool's argument, not by a keyword.
- Deriving call paths from mounted file paths (`MOUNT "./files/export.anyx"` → `files/export/pdf`): it contradicts folders being cosmetic.

## Watch out
- Circular imports are invalid, the user's explicit choice. Types that refer to each other must be declared in the same file. Recursion within one file is fine.
- Sharing a file between two programs gives the same declaration two names in the shell. Match values across programs by shape, not name.

In the code:
- Today's folder loading: `read_schema` and `collect` in `crates/anyx-protocol-generate/src/bin/anyx-protocol-generate.rs`, `ROOT_FILE` and `Schema::parse_files` in `crates/anyx-protocol/src/schema.rs`. Scopes are described in PROTOCOL.md's "Scopes" section.
