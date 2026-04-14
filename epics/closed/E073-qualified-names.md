---
id: "E073"
title: Structured qualified names
created: 2026-04-14
tickets: ["0400", "0401", "0402", "0403"]
---

## Summary

Replace the ad-hoc slash-separated `qualify()` string scheme used by the
DSL expander with a structured `QName` type, per ADR 0034. Migrate all
downstream consumers (interpreter, LSP, SVG renderer) so that namespace
semantics are preserved at the type level instead of being re-parsed at
each use site.

## Acceptance criteria

- [ ] `QName` type defined in a shared location (likely `patches-core` or
      a new `patches-qname` helper) with `bare`, `child`, `is_bare`, and
      `Display` impls.
- [ ] `patches-dsl` expander uses `QName` internally; `qualify()` and
      `child_ns()` are removed.
- [ ] `FlatPatch` module IDs, connection endpoints, pattern bank keys,
      and song names use `QName`.
- [ ] `patches-interpreter` consumes `QName`-keyed data without
      re-splitting strings.
- [ ] `patches-lsp` `ScopeKey` and `patches-svg` tuple IDs use `QName`
      where the value is semantically a qualified name.
- [ ] `cargo test` passes across the workspace; `cargo clippy` clean.
- [ ] All four tickets closed.

## Tickets

| ID | Title |
|----|-------|
| 0400 | Introduce `QName` type |
| 0401 | Migrate DSL expander to `QName` |
| 0402 | Migrate `FlatPatch` and interpreter to `QName` |
| 0403 | Migrate LSP and SVG renderer to `QName` |

## Notes

See ADR 0034. Epic E074 (song sections and play composition) depends on
this work for song-local pattern mangling.
