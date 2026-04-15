---
id: "0426"
title: Extract structural-checks pass from DSL expander
priority: high
created: 2026-04-15
---

## Summary

`patches-dsl::expand` currently interleaves mechanical expansion with
structural validation: unknown param/alias checks (~lines 741–1240),
unknown module-name checks (~1181–1187), unresolved `<param>` refs in
songs (~162–182), and implicit "exactly one patch block" enforcement
(via the parser, no dedicated error). Split this into a distinct
post-expansion pass so stage 3a's error type is separable from stage
3b's binding errors.

## Acceptance criteria

- [ ] New `patches-dsl::structural` module (or equivalent) runs after
      `expand()` and returns `Result<FlatPatch, Vec<StructuralError>>`.
- [ ] `StructuralError` is provenance-carrying and covers: unknown
      param, unknown alias, unknown module name, unresolved `<param>`
      ref, missing/multiple `patch` block, recursive template
      instantiation.
- [ ] `expand.rs` reduced to mechanical expansion; any remaining
      validation left behind is justified in comments.
- [ ] Unit tests in `patches-dsl/tests/structural_tests.rs` cover each
      error variant with provenance assertions.
- [ ] Existing `expand_tests.rs` still passes (success path unchanged).
- [ ] `cargo test -p patches-dsl`, `cargo clippy -p patches-dsl` clean.

## Notes

Keep the FlatPatch shape unchanged. Recursive template instantiation
detection currently lives in `expand.rs`; if it's cheaper to detect
during expansion than post-hoc, produce the error there but route it
through the new `StructuralError` type.
