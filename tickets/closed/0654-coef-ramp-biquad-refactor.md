---
id: "0654"
title: Refactor MonoBiquad + PolyBiquad onto CoefRamp
priority: medium
created: 2026-04-23
epic: E112
depends_on: ["0653"]
---

## Summary

Replace the hand-rolled active/target/delta fields in `MonoBiquad` and
`PolyBiquad` with `CoefRamp<5>` / `PolyCoefRamp<5, 16>` plus
`CoefTargets<5>` / `PolyCoefTargets<5, 16>`. Simpler kernel (no
per-coefficient extras like SVF's `stability_clamp`), so it's the
cleanest migration to do first.

## Acceptance criteria

- [ ] One commit for `MonoBiquad`, one for `PolyBiquad`.
- [ ] Public surface preserved: `begin_ramp(b0t, b1t, b2t, a1t, a2t, interval_recip)`
      still works (thin wrapper that builds `[f32; 5]` and delegates).
      Alternative: change sig to take `[f32; 5]` and fix call sites — decide
      per which produces cleaner module code.
- [ ] Hot/cold field layout preserved: `CoefRamp` in hot region of struct,
      `CoefTargets` after state. Verify by reading the struct definition
      and comparing to current ordering.
- [ ] `tick_all` / `tick` bodies read `self.coefs.active[k]` (or named
      helpers if clearer) with no measurable change in generated SIMD.
- [ ] All existing biquad tests pass (`cargo test -p patches-dsp biquad`).
- [ ] `cargo clippy -p patches-dsp --all-targets` clean.
- [ ] Filter modules (`patches-modules/src/filter/{lowpass,highpass,bandpass}.rs`
      and poly equivalents) still compile and their tests pass.

## Notes

Decide between preserving the 5-positional-arg `begin_ramp` signature or
switching to `[f32; 5]`. The call sites (lowpass/highpass/bandpass) already
destructure from a tuple returned by `compute_biquad_*`. Changing those
helpers to return `[f32; 5]` is probably cleaner, but it's a bigger blast
radius — judgement call during the commit.

Leave the 0655 bench/disasm gate until this is done on PolyBiquad.
