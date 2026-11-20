---
title: "An optional part is sugar: the full form is the method, and shorter forms fill in its defaults"
author: Wongjiahau
date: 2026-11-20
branch: feat/subtyping
cost: { minutes: 6, tokens: 22712 }
id: l-36e77adb
scope: repo
status: active
src: agent
touches:
  - crates/anyx-protocol-generate/src/rust.rs
---

## Decided
A shape's optional part, `find (pattern: Text) [in (paths: LIST Text) DEFAULT in ["."]]`, is sugar over one method, the full form `find _ in _`. Its name, JSON name, guide entry and server handler are the full form's, and the handler always gets every parameter.
- The shorter forms are filled in from each part's `DEFAULT`. The request decoder turns `find "x"` into `find "x" in ["."]`.
- Generated clients get a function per form, and a short one forwards with the defaults filled in: `find(pattern)` returns `this.findIn(pattern, ["."])`. The vendor implements only `find_in` in the server trait.
- Each optional part doubles the client functions: fd's `find` has `find`, `findIn`, `findWith`, `findInWith`.

Why: the user wanted optional parts to be sugar, removed before the checker and the generator see a method. `DEFAULT` is what makes a short form mean the full call. Without it, a short form would be a method of its own with nothing to say what it searches.

## Tried and ruled out
- Expanding the declaration into one method per form, with no `DEFAULT`: a `METHOD` has no body, so `find _` couldn't forward to `find _ in _`. Each form would need its own handler, and the schema's defaults would be read by nothing.
- A generated client that takes every parameter, as for a record field with a `DEFAULT`: the caller would repeat the schema's default.
- A client with `Option` or `?` parameters for left-out parts: the client and the handler would differ in type.

This supersedes the 11-19 learning on how a call with left-out parts reaches the handler. Its syntax, `[part DEFAULT entry]` inside a shape, stands.
