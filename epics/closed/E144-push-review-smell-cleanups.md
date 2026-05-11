---
id: E144
title: Push-review smell cleanups
status: open
created: 2026-05-10
---

## Summary

Small coherence fixes surfaced by code review of the e2beb7d → e7df6d8
push (0857 autosum collapse + 0858 backplane → scratch + 0859
benchmarks). None of these are correctness bugs; they are ad-hoc
structures or parallel APIs that will rot if left.

Independent one-shot tickets; no sequencing.

## Tickets

- 0864 — `EdgeOrigin` is a single-field struct returned from
  `flat_to_layout_input`. Newtype or `usize`.
- 0865 — Two `impl PositionedNode` blocks in `patches-svg/src/layout.rs`.
  Merge.
- 0866 — `NodeHint` is enriched in two passes that must not overwrite
  each other's fields; the contract lives in a code comment.
  Split by enrichment phase or document on the type.
- 0867 — Two-surface API for autosum detection
  (`QName::is_autosum()` + free `is_autosum_name(&str)` +
  `AUTOSUM_PREFIX` const). Drop the free function; consumers call
  `name.starts_with(AUTOSUM_PREFIX)`.

## Out of scope

The big-ticket items from the same review (scratch-first layout, dead
`cycle_slot_start` diagnostic, `with_cycle_only` migration, bench.rs
HWM re-derive) live in E143.
