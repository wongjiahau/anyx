---
title: "A command-line tool joins the anyx shell by answering a flag, and exits after each request"
author: Wongjiahau
date: 2026-11-18
cost: { minutes: 1, tokens: 2648 }
id: l-87857113
scope: repo
status: active
src: agent
---

## Decided
Any executable, in any language, is an anyx protocol tool if it answers a specific flag (working name `--anyx-protocol`) by reading a request on stdin and writing the response on stdout. There is no package format: the tool is just a program on PATH, and the shell learns its schema by asking it.

The tool exits after each request, like an ordinary command. The shell closes stdin after writing the request, so end of input marks the end of the request and no framing is needed.

## Tried and ruled out
- Compiling tools into the shell (Rust `Tool` trait only): closed set, Rust only.
- WebAssembly components as the package: the user wants any program on PATH to be able to join.
- One long-running process per session, like a language server: it needs message framing on the open pipe, and tools should behave like ordinary commands.

## Watch out
- The shell must only send the flag to programs it was told to use (`USE ALL FROM rg`, or a call `rg / …`), never probe every program on PATH: an unknown flag may be read as a file name.
- Built-in `SCHEMA` returns the guide, not the schema source, so discovery needs a call that returns the schema files.
- Each request pays the tool's startup time (milliseconds natively, 50-300 ms for Python or Node), and state between requests lives in files, as with any command.
- Effect markers such as READ ONLY are the tool's claim; the shell does not enforce them.
