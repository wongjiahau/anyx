---
title: "Anyfix names should read as natural English, even with slots side by side"
author: Wongjiahau
date: 2026-11-24
id: l-3f9a6c21
scope: repo
status: active
src: agent
---

## Decided
- Name a declaration so that a use of it reads as natural English: `Vector of (three) (Natural)`, not `Vector of (Natural) length (three)`.
- Two slots side by side are fine. The reader doesn't need a label word between them to tell the arguments apart: the signature and the agent writing the code keep track of which is which, and readers get the plain sentence.
- Why: anyfix exists to make reading natural. A label word added only to say "this slot is the length" makes every use read worse, to help with something tools already handle.

In the code: `core/examples/vector.anyx` declares `DATA Vector of (length: Natural) (item: Type)`.
