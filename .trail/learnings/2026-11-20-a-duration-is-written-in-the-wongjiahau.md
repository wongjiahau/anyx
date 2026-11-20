---
title: "A duration is written in the plural only, once it is a prelude function"
author: Wongjiahau
date: 2026-11-20
branch: feat/subtyping
cost: { minutes: 6, tokens: 34798 }
id: l-1e4c2cea
scope: repo
status: active
src: agent
touches:
  - crates/anyx-protocol/request.grammar.anyx
---

## Decided
When `Duration` becomes an intrinsic prelude function with optional parts (#54), each unit is one fixed word in its shape: `[(hours: Integer) hours DEFAULT 0 hours]`. A duration is written `1 hours 30 minutes`. `1 hour` is rejected with a hint to write `1 hours`, and the encoder and the guide write the plural everywhere, so the encoder's output reads back.

Why: which function a call means is a lookup by its shape in the schema. Accepting `hour` for `hours` would reinterpret the tree, which the rule against custom parsing over the generated parser forbids. The singular used to read, through the request grammar's own `duration` rule and `time.rs`, which went.

## Tried and ruled out
- A choice of words within a shape, `{day|days}`: the declaration parses as `_ _` (two slots side by side) while a call parses as `_ day`; it gives no agreement with the count (`2 day` reads); and methods would get synonyms such as `{remove|delete}`. Recorded as experimental in #72.
- Keeping `Duration` intrinsic and special: it drops `1 hours 30 minutes` equalling `90 minutes`, which #54 wants.
