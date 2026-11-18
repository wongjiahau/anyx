---
title: "A protocol tool calls the library behind a command, never the command line itself"
author: Wongjiahau
date: 2026-11-18
cost: { minutes: 3, tokens: 5711 }
id: l-fdba9b0b
scope: repo
status: active
src: agent
---

## Decided
The anyx shell is a new kind of command line, not a layer over existing ones. A tool that speaks the anyx protocol (for example a search like ripgrep, or a file finder like fd) is written against the libraries the command is built on, not by running the command and reading its output and exit code.

The shell takes over every job of a traditional CLI: argument parsing becomes a typed call checked against the schema, `--help` becomes SCHEMA and completion, text on stdout becomes typed values, exit codes and stderr become declared ERRORS / INTERNAL ERROR / INVALID REQUEST, pipes become nested calls, and flags with defaults become record fields with DEFAULT.

## Tried and ruled out
- Wrapping the CLI and mapping exit codes to protocol errors (for example rg's 0/1/2). An exit code is too coarse (rg's 2 covers a bad regex, a missing path and an unreadable file), stderr text is not a stable interface, and it keeps the text-and-argv interface the shell is meant to replace.

## Watch out
- A declared ERROR still means the call changed nothing; a failure the tool can't classify is INTERNAL ERROR.
- Text must be Unicode, but paths and matched lines may not be; decide per field between rejecting, Bytes, or lossy conversion.
- Results are one whole response, with no streaming, so search-like methods need a limit and a truncated flag.

In the code: ripgrep's libraries are grep-searcher, grep-regex, grep-matcher and ignore (directory walking with .gitignore); fd's core is the ignore crate plus globset/regex. Tools implement anyx-shell's `Tool` trait (schema + invoke).
