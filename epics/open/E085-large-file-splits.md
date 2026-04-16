---
id: "E085"
title: Break up files over 600 lines
created: 2026-04-16
status: open
tickets: ["0474", "0475", "0476", "0477", "0478", "0479", "0480", "0481", "0482", "0483", "0484", "0485", "0486", "0487", "0488", "0489", "0490", "0491"]
---

## Summary

36 `.rs` files in the workspace are over 600 lines; the longest is
1510. Several recent cleanups (E082/E083) validated the pattern of
pulling embedded test modules into sibling `tests.rs` files (0459
split `workspace.rs`) and splitting expand/analysis into phase-tagged
modules. The bulk of remaining bloat falls into three shapes:

1. **Test-dominant** files where `#[cfg(test)] mod tests` accounts for
   300–800 lines and dominates the file view when opening the module.
   Mechanical extraction, same pattern as 0459.
2. **Impl-dominant** monolithic modules where a natural structural
   split exists (parser by grammar node, ast_builder by AST section,
   workspace by concern, mixer by variant type, cables by port type).
3. **Test-file bloat** where integration test files accumulate many
   categories and benefit from category subdirs (same pattern just
   applied to `expand_tests`).

This epic covers **tier A only**: mechanical test extraction for the
18 source files where the inline test module is ≥270 lines. Impl
splits (tier B, 15 tickets) and test-file category splits (tier C, 5
tickets) are follow-on epics once tier A lands and file sizes are
rebaselined.

No behavioural change. Each ticket is small, independent, and leaves
the public surface of the crate untouched.

## Acceptance criteria

- [ ] All 18 tickets (0472–0489) closed.
- [ ] Every source file listed below has its inline `mod tests` moved
      to a sibling `tests.rs` under a newly created `foo/` directory
      (or equivalent `#[path]` layout if preferred per-ticket).
- [ ] `cargo build`, `cargo test`, `cargo clippy` clean at each
      ticket boundary.
- [ ] No public API changes.
- [ ] A second histogram pass (`wc -l` over `**/*.rs`) shows no source
      file over ~600 lines purely due to inline tests.

## Notes

Pattern reference: ticket 0459 split `patches-lsp/src/workspace.rs`
into `workspace/mod.rs` + `workspace/tests.rs`. Follow the same
convention: convert `foo.rs` to `foo/mod.rs` + `foo/tests.rs`, with
the parent module declaring `#[cfg(test)] mod tests;`.

Out of scope for this epic:

- Further impl-side splits (tracked as follow-on E086).
- Test-file category splits for `tests/*.rs` (tracked as E087).
- Any behaviour change, renaming, or public-API adjustment.
