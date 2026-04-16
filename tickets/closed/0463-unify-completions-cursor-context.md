---
id: "0463"
title: Unify parsed-input completions on CursorContext
priority: high
created: 2026-04-15
---

## Summary

`patches-lsp/src/completions.rs` currently has three layers:

1. **classify_cursor fast path** (lines 29-33) — fires only for
   `ModuleType`, marked as direction-of-travel.
2. **try_completion_from_node** (lines 85-158) — ancestor walk
   over the parse tree handling ParamBlock, ShapeBlock, PortRef,
   song_row, connection.
3. **scan_backward_for_context** (lines 638-711) — text scan for
   incomplete input tree-sitter hasn't accepted (`osc.`,
   `module x :`, `$.`, `mix.out[`).

Layers 1 and 2 solve the same problem (parsed-input cursor
classification) in different idioms. Layer 3 solves a different
problem (recovery when tree-sitter has no node) and stays.

`CursorContext` already declares `ParamBlock`/`ShapeBlock`/
`PortRef` variants (`tree_nav.rs:46-54`) but they're
`#[allow(dead_code)]` because no caller dispatches on them.

## Acceptance criteria

- [ ] `CursorContext` extended (or sub-discriminators added) to
      cover the parsed-input cases currently handled by
      `try_completion_from_node`: ParamBlock with `@` /
      MasterSequencer-`song:` / general-parameters
      sub-cases; ShapeBlock; PortRef with connection-side and
      `$`-template detection; song_row.
- [ ] `compute_completions` dispatches fully on `CursorContext`
      for parsed input.
- [ ] `try_completion_from_node` deleted.
- [ ] `scan_backward_for_context` retained as the explicit
      incomplete-input fallback, with a module-level docstring
      explaining why both paths exist (tree-sitter can't classify
      a node that doesn't exist yet).
- [ ] `#[allow(dead_code)]` removed from `CursorContext` variants
      that are now used.
- [ ] Existing completion test fixtures pass unchanged.
- [ ] `cargo build`, `cargo test -p patches-lsp`, `cargo clippy` clean.

## Out of scope

Folding `scan_backward_for_context` into `classify_cursor`. That
would require either ERROR-node text inspection or richer
tree-sitter error recovery — separate concern.

## Notes

E084. Direct follow-on to 0457. The classifier was built to be
shared; finishing the parsed-input migration removes the
alternate idiom and leaves the backward scanner as a single,
documented fallback rather than an undocumented twin.
