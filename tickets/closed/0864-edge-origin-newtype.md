---
id: "0864"
title: Simplify `EdgeOrigin` — newtype or `usize`
priority: low
created: 2026-05-10
epic: E144
---

## Summary

`patches-svg/src/flat_to_layout.rs::EdgeOrigin` (added 0857) is a
`Debug, Clone, Copy` struct with one field `conn_idx: usize`. The
struct-with-field shape implies extensibility that isn't there; the
return signature is awkward (`(Vec<LayoutNode>, Vec<LayoutEdge>,
Vec<EdgeOrigin>)`); the `enrich_edge_hints` call site reads
`origin.conn_idx` for the only field there is.

Pick one of:

1. Newtype: `pub struct EdgeOrigin(pub usize)`. Keeps the named type
   for documentation value; drop the named field.
2. Type alias: `pub type EdgeOrigin = usize`. Strips the wrapper
   entirely; the slice type carries the intent through the docstring.

Either is honest; the current shape is the one that isn't.

## Acceptance criteria

- [ ] `EdgeOrigin` reduced to a newtype or type alias.
- [ ] Doc comment on the chosen shape explains why the index is
      threaded separately from `LayoutEdge` (auto-Sum collapse breaks
      the edge↔connection 1:1 correspondence).
- [ ] `just push` clean.

## Notes

Newtype is the slight preference: searches for `EdgeOrigin` keep
finding the doc comment.
