---
id: "0755"
title: Replace FDN reverb golden-byte test with kernel property assertions
priority: medium
created: 2026-04-29
---

## Summary

`patches-integration-tests/tests/fdn_reverb_golden.rs` compares an FDN impulse response byte-for-byte against a 2048-sample golden file (`patches-integration-tests/golden/fdn_reverb_impulse.bin`) at 1e-4 tolerance. It's brittle to any benign DSP change (filter retuning, matrix permutation, oversample tweak) and weak as a diagnostic — a failure says "bytes differ", not "decay too fast" or "energy unbounded".

The test also doesn't belong in `patches-integration-tests`. It already drives `FdnReverb` via `ModuleHarness` in isolation — no engine, no planner. The FDN kernel lives in `patches-modules/src/fdn_reverb/` (`processor.rs`, `line.rs`, `matrix.rs`).

Goal: delete the golden test and the golden binary; replace with property-based assertions in `patches-modules/src/fdn_reverb/tests.rs` driven directly against the FDN kernel — no `ModuleHarness`, no `Module` wrapper, no cable I/O.

## Acceptance criteria

- [ ] `patches-integration-tests/tests/fdn_reverb_golden.rs` deleted.
- [ ] `patches-integration-tests/golden/fdn_reverb_impulse.bin` deleted (and `golden/` dir if now empty).
- [ ] New tests in `patches-modules/src/fdn_reverb/tests.rs` construct the FDN kernel struct directly and feed samples; no `ModuleHarness`, no `Module::process`.
- [ ] Property assertions cover the real invariants, e.g.:
  - finite output (no NaN/Inf) over N samples on unit impulse
  - windowed RMS decays past the early build-up
  - RT60 estimate within an expected band for the `plate` archetype
  - early reflections non-zero before the late-field smear
  - bounded peak for unit impulse input
- [ ] Tolerances chosen with margin so minor DSP retuning passes but real regressions fail; each assertion commented with the invariant it encodes.
- [ ] `cargo test -p patches-modules` and `cargo test -p patches-integration-tests` pass; `cargo clippy` clean.

## Notes

- If the FDN kernel struct is private to `processor.rs`, expose it `pub(crate)` for tests rather than routing through the `Module` trait.
- Calibrate thresholds from the current kernel's behaviour with margin — don't lock in exact numbers (that just reinvents a golden file).
- `poly_filters_survive_plan_reload` is **not** in scope here; it's a planner-reload sentinel, moved to 0756.
