---
id: "0129"
title: `PolyResonantHighpass` and `PolyResonantBandpass` modules
priority: medium
created: 2026-03-18
epic: "E024"
depends_on: ["0128"]
---

## Summary

Add `PolyResonantHighpass` and `PolyResonantBandpass` to `poly_filter.rs`,
following the template established by `PolyResonantLowpass` in T-0128.
The only differences from the lowpass are the module name, parameter names
(bandpass uses `center`/`bandwidth_q`), and the coefficient formula called at
update time.

## Acceptance criteria

### `PolyResonantHighpass`

- [ ] `pub struct PolyResonantHighpass` in `poly_filter.rs`, registered under
      `"PolyHighpass"`.
- [ ] Identical field layout and port descriptor to `PolyResonantLowpass`.
      Parameters: `cutoff` (Float, 20–20 000, default 1000), `resonance`
      (Float, 0–1, default 0), `saturate` (Bool, default false).
- [ ] Coefficient recomputation calls `compute_biquad_highpass` (from T-0127)
      in place of `compute_biquad_lowpass`. All snap/ramp/static logic is
      otherwise identical.
- [ ] Unit tests:
      - `poly_highpass_attenuates_below_cutoff`: all 16 voices peak < 0.05 at
        cutoff / 10 after settling.
      - `poly_highpass_passes_above_cutoff`: all 16 voices peak > 0.9 near
        Nyquist / 2 after settling.
      - `poly_highpass_voices_are_independent_with_cv`: analogous to the
        lowpass CV independence test, with the direction of the effect inverted
        (higher cutoff CV → less signal passed at a frequency just above the
        base cutoff).

### `PolyResonantBandpass`

- [ ] `pub struct PolyResonantBandpass` in `poly_filter.rs`, registered under
      `"PolyBandpass"`.
- [ ] Parameters: `center` (Float, 20–20 000, default 1000), `bandwidth_q`
      (Float, 0.1–20, default 1.0), `saturate` (Bool, default false).
      Input port `cutoff_cv` is named `center_cv` in the descriptor (same
      convention as the mono `ResonantBandpass` from T-0127).
- [ ] Coefficient recomputation calls `compute_biquad_bandpass(center, bandwidth_q, sr)`
      for the static path and per-voice effective values on the CV path:
      `effective_center = (center * center_cv[i].exp2()).clamp(20.0, sr*0.499)`,
      `effective_q = (bandwidth_q + bandwidth_q_cv[i]).clamp(0.1, 20.0)`.
- [ ] Unit tests:
      - `poly_bandpass_attenuates_far_from_center`: all 16 voices peak < 0.1
        at center / 10 and at center × 10.
      - `poly_bandpass_passes_at_center`: all 16 voices peak > 0.8 at the
        centre frequency after settling.
      - `poly_bandpass_narrow_q_is_narrower_than_wide_q`: with Q = 8, the
        off-centre peak (one octave away) is lower than with Q = 0.5.
      - `poly_bandpass_voices_are_independent_with_cv`: per-voice `center_cv`
        produces measurably different bandpass centres across voices.

### Both modules

- [ ] Registered in the default module registry in `lib.rs`.
- [ ] `cargo build`, `cargo test`, `cargo clippy` pass with no new warnings.

## Notes

`compute_biquad_highpass` and `compute_biquad_bandpass` are defined in
`filter.rs` (T-0127). Make them `pub(crate)` if needed so `poly_filter.rs`
can call them; they should not be duplicated.

The structure of `process` for both modules is copy-identical to
`PolyResonantLowpass` except for the coefficient function call. If this
repetition grows uncomfortable a private macro or a generic helper function
taking the coefficient function as a parameter could be introduced — but only
if it clearly simplifies the code. Do not add abstraction preemptively.
