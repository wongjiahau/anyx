---
title: "The shell imports a tool under a name, and opens its methods in a separate directive"
author: Wongjiahau
date: 2026-11-18
cost: { minutes: 3, tokens: 8196 }
id: l-66b274e9
scope: repo
status: active
src: agent
touches:
  - crates/anyx-shell/shell.grammar.anyx
---

## Decided
The anyx shell is one of several sources of protocol tools, so a session brings a tool in with a directive that names the source and chooses the tool's name: `USE rg FROM COMMAND "rg"`, `USE notes FROM URL "…"`. The source is a choice, so a new kind of source is a new variant. Opening the tool's methods is a separate directive that takes only a name: `USE ALL FROM rg`.

Why separate: when two opened tools have a method of the same shape, a call is fixed by writing the tool's name in front (`rg / search …`). An import that also opened, `USE ALL FROM COMMAND "rg"`, would leave the tool with no name to write.

Why `USE` and not a new `TOOL` keyword: it reads naturally, and every other `USE` (shell openings, schema-file imports) already brings names into scope. A name is lowercase and `ALL` is a keyword, so the grammar tells `USE rg FROM …` from `USE ALL FROM …`.

## Watch out
- The directive chooses the name, not the tool's `PROTOCOL` name, so two vendors' tools both called `search` can live in one session and a script reads without running anything.
- Importing is when the shell asks for the schema, so a missing program or broken schema is reported on that line, before any call runs.
- Built-in tools such as `environment` and `boolean` are tools already imported when a session starts.

In the code: the directives go in crates/anyx-shell/shell.grammar.anyx and are read from the generated tree as-is.
