---
id: "0127"
title: Mono `ResonantHighpass` and `ResonantBandpass` filters
priority: medium
created: 2026-03-18
epic: "E024"
depends_on: ["0125"]
---

## Summary

Add two new mono filter modules to `patches-modules/src/filter.rs` using the
`MonoBiquad` kernel from T-0125: `ResonantHighpass` (RBJ highpass) and
`ResonantBandpass` (RBJ bandpass, constant 0 dB peak gain). Both modules share
the same port layout and static/CV path structure as the refactored
`ResonantLowpass`; they differ only in their coefficient formula.

## Acceptance criteria

### `ResonantHighpass`

- [ ] `pub struct ResonantHighpass` in `filter.rs` with the same fields as the
      refactored `ResonantLowpass` (parameters `cutoff`/`resonance`/`saturate`,
      `MonoBiquad`, port fields), registered under the name `"Highpass"`.
- [ ] `compute_biquad_highpass(cutoff_hz, resonance, sample_rate) -> (f32,f32,f32,f32,f32)`
      implements the RBJ highpass design equations:
      - `alpha = sin(w0) / (2·Q)` where `Q = resonance_to_q(resonance)`
      - `b0 =  (1 + cos(w0)) / 2 / a0`
      - `b1 = -(1 + cos(w0)) / a0`
      - `b2 =  (1 + cos(w0)) / 2 / a0`
      - `a1`, `a2` identical to the lowpass formula
- [ ] Port layout, parameter ranges, CV semantics (`cutoff_cv` as V/oct,
      `resonance_cv` as additive), and `saturate` behaviour are identical to
      `ResonantLowpass`.
- [ ] Unit tests:
      - `highpass_attenuates_below_cutoff`: peak at 10× below cutoff < 0.05
        after settling.
      - `highpass_passes_above_cutoff`: peak near Nyquist / 2 is > 0.9 after
        settling.
      - `highpass_resonance_boosts_near_cutoff`: resonant peak at cutoff
        exceeds flat peak by ≥ 1.5×.
      - `highpass_cutoff_cv_shifts_cutoff`: +1 V/oct raises cutoff, reducing
        attenuation at a test frequency above the unmodulated cutoff.

### `ResonantBandpass`

- [ ] `pub struct ResonantBandpass` in `filter.rs`, registered under the name
      `"Bandpass"`.
- [ ] `compute_biquad_bandpass(center_hz, bandwidth_q, sample_rate) -> (f32,f32,f32,f32,f32)`
      implements the RBJ bandpass (constant 0 dB peak gain) equations:
      - `alpha = sin(w0) / (2·Q)` where `Q = bandwidth_q`
      - `b0 =  alpha / a0`
      - `b1 =  0.0`
      - `b2 = -alpha / a0`
      - `a1`, `a2` identical to the lowpass formula
- [ ] Parameters:
      - `center` (Float, min 20.0, max 20 000.0, default 1000.0) — centre
        frequency in Hz, equivalent role to `cutoff` on LP/HP.
      - `bandwidth_q` (Float, min 0.1, max 20.0, default 1.0) — filter Q;
        higher values narrow the passband. Exposed directly rather than through
        `resonance_to_q` because Q *is* the perceptually meaningful parameter
        for a bandpass.
      - `saturate` (Bool, default false) — same meaning as on LP/HP.
- [ ] Input port `cutoff_cv` is renamed `center_cv` in the descriptor to
      reflect the parameter it modulates; V/oct semantics are unchanged.
      `resonance_cv` modulates `bandwidth_q` additively (clamped to [0.1, 20.0]).
- [ ] Unit tests:
      - `bandpass_attenuates_far_below_center`: peak at center / 10 < 0.1.
      - `bandpass_attenuates_far_above_center`: peak at center × 10 < 0.1.
      - `bandpass_passes_at_center`: peak at center frequency > 0.8 after
        settling.
      - `bandpass_narrow_q_is_narrower_than_wide_q`: with Q = 10, the peak at
        center ± one octave is lower than with Q = 0.5.
      - `bandpass_center_cv_shifts_center`: +1 V/oct doubles the effective
        centre, increasing output at a frequency one octave above the base.

### Both modules

- [ ] Registered in `patches-modules/src/lib.rs` default registry.
- [ ] `cargo build`, `cargo test`, `cargo clippy` pass with no new warnings.

## Notes

`resonance_to_q` is not used by `ResonantBandpass`. It remains a private
helper in `filter.rs` for LP and HP only.

The bandpass `b1 = 0` means the first state update contains no direct `x`
term: `s1 = 0·x − a1·fb + s2 = −a1·fb + s2`. `MonoBiquad::tick` handles this
naturally since it stores `b1` as a coefficient — no special-casing needed.
