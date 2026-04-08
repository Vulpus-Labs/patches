---
id: "0152"
title: Extend ModuleHarness to support poly I/O
epic: E025
priority: medium
created: 2026-03-20
---

## Summary

`ModuleHarness` in `patches-core/src/test_support/harness.rs` only exposes `set_mono` and `read_mono`. Poly modules (`PolyOsc`, `PolyAdsr`, poly filters, etc.) must be tested via integration tests in `patches-integration-tests`, which is slower, harder to isolate, and requires wiring up a full `HeadlessEngine`. Unit-level coverage for poly modules is significantly weaker than for mono modules.

## Acceptance criteria

- [ ] Add `set_poly(port: &str, value: [f64; 16])` to `ModuleHarness`.
- [ ] Add `read_poly(port: &str) -> [f64; 16]` to `ModuleHarness`.
- [ ] Add `read_poly_voice(port: &str, voice: usize) -> f64` as a convenience helper.
- [ ] At least one existing poly module (`PolyOsc` or `PolyAdsr`) gains a unit test using the new harness methods that would otherwise require an integration test.
- [ ] `cargo clippy` clean; `cargo test` passes.

## Notes

The harness already manages the `CablePool` and port wiring internally. Adding poly support is likely a small addition to the same machinery; the main change is plumbing `[CableValue::Poly([f64;16])]` into the relevant cable slots.
