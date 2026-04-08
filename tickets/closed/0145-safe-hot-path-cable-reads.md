---
id: "0145"
title: Replace unreachable!() in cable hot path with safe fallbacks
epic: E024
priority: high
created: 2026-03-20
---

## Summary

`MonoInput::read()`, `PolyInput::read()`, `CablePool::read_mono()`, and `CablePool::read_poly()` all use `unreachable!()` to reject mismatched cable kinds (e.g. a `Poly` value on a mono input). Because these methods are called every sample on the audio thread, any planner bug or malformed hot-reload that violates the invariant will panic the audio thread and cause a hard dropout.

The comment on each says "graph validation should prevent this", but there is no compile-time or runtime enforcement boundary that makes that guarantee ironclad across future changes.

## Acceptance criteria

- [ ] In `patches-core/src/cables.rs`, `MonoInput::read()` and `PolyInput::read()` return a silent zero value (`0.0` / `[0.0; 16]`) on kind mismatch rather than calling `unreachable!()`.
- [ ] In `patches-core/src/cable_pool.rs`, `CablePool::read_mono()` and `CablePool::read_poly()` do the same.
- [ ] A `debug_assert!` or `#[cfg(debug_assertions)]` check is added in each site so mismatches are caught immediately in test/debug builds but degrade gracefully in release.
- [ ] Existing tests still pass; add a test that proves the fallback path returns zero (not a panic) when a kind mismatch occurs.
- [ ] `cargo clippy` clean; `cargo test` passes.

## Notes

Precedent for silent-fallback-with-debug-assert is common in real-time audio code (e.g. JUCE). The mismatch should never happen in a correct build, but defensive handling protects live performance.

The relevant sites:
- `patches-core/src/cables.rs:91-93` (`MonoInput::read`)
- `patches-core/src/cables.rs:136-138` (`PolyInput::read`)
- `patches-core/src/cable_pool.rs:30-32` (`read_mono`)
- `patches-core/src/cable_pool.rs:45-47` (`read_poly`)
