---
id: "0499"
title: Split patches-lsp completions.rs by context
priority: medium
created: 2026-04-16
---

## Summary

`patches-lsp/src/completions.rs` is 821 lines. Dispatch plus a
per-context completer function for every CursorContext variant,
plus a backward-scan fallback for incomplete input.

## Acceptance criteria

- [ ] Convert to `completions/mod.rs` with submodules:
      `module_types.rs`, `params.rs`, `ports.rs`, `shape.rs`
      (shape args / aliases / at-block), `backward_scan.rs`.
- [ ] `compute_completions` dispatcher stays in `mod.rs`.
- [ ] Each submodule under ~250 lines; `mod.rs` under ~200.
- [ ] `cargo build -p patches-lsp`, `cargo test -p patches-lsp`,
      `cargo clippy` clean.

## Notes

E086. No behaviour change.
