---
title: "Nullable is a union with the variant null, not a Null named type"
author: Wongjiahau
date: 2026-11-19
branch: feat/type-alias
cost: { minutes: 1, tokens: 1265 }
id: l-3ec39271
scope: repo
status: active
src: agent
touches:
  - crates/anyx-protocol/prelude.protocol.anyx
---

## Decided
- `?` is `TYPE ALIAS (Item: TYPE) ? : UNION [Item, null]`, where `null` is a lowercase variant, as `false` and `true` are. There is no `NAMED TYPE Null`.
- In anyx text nothing differs: the value is written `null` either way, and a second `null` in a union's nesting is still a repeated form, so `Depth??` is rejected.
- A plain JSON `null` is a question of JSON tags, where any variant could choose its JSON form, not a reason for a separate type.
- `NAMED TYPE` is its own stage. The union stage accepts variants as members from the start, so the `CHOICE` stage only replaces the keyword.

## Tried and ruled out
- `NAMED TYPE Null` with `UNION [Item, Null]`, as #51 first had it. It adds a type and an intrinsic only to get a JSON `null`, and ties the union stage to the named type stage.

In the code: crates/anyx-protocol/prelude.protocol.anyx (the `?` choice). Issues #51, #56, #57, #58.
