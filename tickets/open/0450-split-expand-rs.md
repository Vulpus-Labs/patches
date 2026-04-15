---
id: "0450"
title: Split expand.rs into connection and composition modules
priority: high
created: 2026-04-15
---

## Summary

`patches-dsl/src/expand.rs` is ~1960 lines and fuses four conceptual
phases into one file: (a) template recursion + parameter binding,
(b) connection flattening, (c) song/pattern assembly, (d) boundary-port
mapping. The single `expand_body` at lines 1021–1162 runs four nested
passes over the same statement slice with six mutually-referenced output
collections. Split the file so each phase is graspable on its own.

## Acceptance criteria

- [ ] New module `patches-dsl/src/expand/connection.rs` owns
      `resolve_from`, `resolve_to`, connection flattening, scale
      composition, and port-index resolution.
- [ ] New module `patches-dsl/src/expand/composition.rs` owns
      `flatten_song`, `resolve_songs`, pattern assembly, and
      inline-pattern tracking.
- [ ] `patches-dsl/src/expand.rs` (or `expand/mod.rs`) retains template
      recursion, parameter binding, and orchestration; `expand_body`
      splits into per-pass functions or a runner with explicit state.
- [ ] Public API of the crate unchanged; only internal module paths
      move.
- [ ] `cargo build`, `cargo test`, `cargo clippy` clean.

## Notes

Seen during E083 review. Boundary-port mapping (TemplatePorts /
PortEntry) may stay with composition or move to a small `ports` helper
— pick whichever keeps the four-pass split clean. Pure reorganisation;
no behavioural change.
