---
title: "General types get general names, not names from where they're first used"
author: Wongjiahau
date: 2026-11-28
id: l-8f2d6b17
scope: repo
status: active
src: agent
---

## Decided
- A general type, or one of its values, is named for what it is, not for the first place it's used. `Nothing`'s one value is `nothing`, not `done`. `done` assumed the value is the result of an endpoint that did something. In a field or a list item it would read wrong.
- A domain's reading belongs to the domain's own names. A successful reply reads `answered (nothing)`; if an API wants "it worked" spelled out, that is the API's own type.

## Watch out
- Watch the prelude (`Boolean`, `Nothing`, `Optional`, `List`) and the generator's special cases for it. A name picked so one example reads nicely leaks that example's domain into every schema.

In the code: `crates/anyx-core-generate/src/client.rs` and `rust.rs` spell the prelude's constructors (`nothing` → `null` / `()`), and `tests/fixtures/endpoints.anyx` declares `DATA Nothing WITH [nothing]`.
