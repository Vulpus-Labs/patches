---
id: "0532"
title: Split patches-lsp workspace/tests.rs by category
priority: medium
created: 2026-04-17
epic: E090
---

## Summary

[patches-lsp/src/workspace/tests.rs](../../patches-lsp/src/workspace/tests.rs)
is 1222 lines — the largest test file in the workspace. Tests cover
several orthogonal axes of `DocumentWorkspace` behaviour: include-cycle
diagnostics, diamond / shared loads, template visibility across
includes, disk/editor change cascades, flatten/parse robustness,
hover, and per-document lifecycle events.

## Acceptance criteria

- [ ] Reduce `src/workspace/tests.rs` to a stub that declares
      `mod tests_impl;` (or similar) pointing at a `tests/` sibling
      directory — the `src/workspace/mod.rs` side keeps
      `#[cfg(test)] mod tests;` unchanged.
- [ ] `src/workspace/tests/mod.rs` declares category submodules and
      hosts any shared fixtures / helpers (the `new`/`write`/`uri`
      test harness at the top).
- [ ] Each category file contains the tests verbatim, grouped by
      prefix / subject. Suggested axes (final naming the ticket's
      call):
      - `cycles.rs` — cycle detection (`cycle_*`, `self_include_*`,
        `missing_include_*`)
      - `includes.rs` — diamond, shared loads, include-graph
      - `templates.rs` — template visibility from includes
      - `propagation.rs` — disk-change / editor-buffer cascades
      - `flatten.rs` — broken-syntax and flatten-cache invalidation
      - `hover.rs` — hover-related workspace tests
- [ ] `cargo test -p patches-lsp` passes with the same test count as
      before.
- [ ] `cargo build -p patches-lsp`, `cargo clippy` clean.

## Notes

E090. Pattern: E087 but for `src/**/tests.rs`. No test logic edits.
