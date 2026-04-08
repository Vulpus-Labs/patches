---
id: "0147"
title: Replace parser unwrap() calls with proper error propagation
epic: E024
priority: high
created: 2026-03-20
---

## Summary

The PEG parser in `patches-dsl/src/parser.rs` contains approximately 15 `.unwrap()` calls that assume the grammar guarantees a parseable token (e.g. a `float_lit` rule always yields a valid `f64`). While the grammar may make this true today, there is no proof obligation enforced at compile time. A grammar regression or an unexpected code path reaching these sites causes a panic on the control thread when loading a patch file.

Example sites:
- Line 106: `inner.as_str().parse().unwrap()` (float_lit)
- Line 107: `inner.as_str().parse().unwrap()` (int_lit)
- Several more in identifier, port-ref, and connection rule handlers

## Acceptance criteria

- [ ] All `.parse().unwrap()` calls in `parser.rs` are replaced with `?` propagation or explicit `map_err` returning a typed `ParseError` variant.
- [ ] The parser's public return type reflects the error (already `Result<_, pest::error::Error<_>>` or similar — extend to cover parse conversion failures).
- [ ] No new `unwrap()` or `expect()` introduced in library code.
- [ ] Existing parser tests pass; add at least one test that feeds a grammar-valid but semantically invalid literal (e.g. a float that overflows `f64`) and verifies a clean error rather than a panic.
- [ ] `cargo clippy` clean; `cargo test` passes.

## Notes

The control thread is not safety-critical in the way the audio thread is, but a panic on a hot-reload crashes `patches-player` mid-performance. Proper error propagation also enables user-facing error messages on invalid DSL files.
