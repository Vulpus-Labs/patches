---
id: "0246"
title: Apply ModuleHarness helpers across patches-modules tests
priority: low
created: 2026-04-01
---

## Summary

The `ModuleHarness` now provides `disconnect_inputs()`, `measure_rms()`,
`measure_peak()`, and `assert_output_bounded()`. The macros `assert_attenuated!`
and `assert_passes!` are available from `patches-core/test_support`. Apply these
across `patches-modules` tests to reduce boilerplate and clarify intent.

## Acceptance criteria

### CV disconnection

- [ ] `filter.rs` — replace repeated `h.disconnect_input("voct");
      h.disconnect_input("fm"); h.disconnect_input("resonance_cv");` with
      `h.disconnect_inputs(&["voct", "fm", "resonance_cv"]);`.
- [ ] `oscillator.rs` — same pattern for `voct`/`fm` disconnection.
- [ ] `lfo.rs` — replace `disconnect_all_inputs()` calls that only need to
      disconnect CV inputs with `disconnect_inputs(&[...])` where the intent
      is clearer.

### Measurement helpers

- [ ] `filter.rs` — replace local `measure_peak()` / `h_measure_peak_with_cv()`
      with `h.measure_peak(ticks, output)` where the local version is a thin
      wrapper.
- [ ] `noise.rs` — replace inline `for v in h.run_mono(...) { assert!(...) }`
      loops with `h.assert_output_bounded(n, output, -1.0, 1.0)`.

### Semantic assertions

- [ ] `filter.rs` — replace `assert!(peak < 0.05, ...)` with
      `assert_attenuated!(peak, 0.05)` and `assert!(peak > 0.5, ...)` with
      `assert_passes!(peak, 0.5)`.
- [ ] `poly_filter.rs` — same where applicable.

### Verification

- [ ] All tests pass, zero clippy warnings.

## Notes

Don't force the helpers where they don't fit. Some filter tests have custom
measurement patterns (e.g. feeding CV while measuring) that need their own
logic. The goal is to replace the *duplicated* patterns, not to abstract every
assertion.

`poly_filter.rs` uses manual `CablePool` wrangling rather than `ModuleHarness`.
If the manual approach is needed for poly testing, leave it but consider
wrapping the pool setup in a local helper to reduce noise. A
`PolyTestHarness` is out of scope for this ticket.
