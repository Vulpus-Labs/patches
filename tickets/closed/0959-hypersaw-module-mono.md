---
id: "0959"
title: HyperSaw — mono detuned-saw oscillator module (patches-modules)
priority: medium
created: 2026-05-27
depends_on: ["0958"]
---

## Summary

Add the `HyperSaw` mono module to `patches-modules/src/osc/`, wrapping one
voice-batched `HyperSawCore` (ticket 0958) and driving **voice lane 0 only**
(the other 15 lanes idle; mono isn't the perf case — ADR 0078). It owns the
control-rate maths: spread → detune ratios, `frequency`/`voct`/`fm` → base
increment, density → per-copy gains, mix → centre/side balance — all computed in
`periodic_update` (for lane 0) and pushed via `core.update`. Per-sample
`process` calls `core.process(&mut out)` and reads `out[0]`. Design: **ADR
0078**.

## Module surface

```text
name: "HyperSaw", axes: [CHANNELS]   // CHANNELS=1 for mono, per osc convention
inputs (mono):  voct, fm, spread_cv, density_cv, mix_cv
outputs (mono): out
params:
  frequency: Float  { min: -4.0, max: 12.0, default: 0.0 }   // v/oct from C0
  fm_type:   Enum<OscFmType>  { linear | logarithmic }        // reuse osc enum
  spread:    Float  { min: 0.0, max: 1.0, default: 0.3 }
  density:   Float  { min: 0.0, max: 1.0, default: 1.0 }      // → ×4 pairs
  mix:       Float  { min: 0.0, max: 1.0, default: 0.7 }      // centre↔side
```

Reuse `common::frequency` (`MonoFrequencyConverter`, `C0_FREQ`, `FMMode`) and
`OscFmType` from `osc::oscillator`. No `sync`, no `phase_mod`, no `reset_out`
(ADR 0078 §6). No drift in v1 (not requested).

## Control-rate maths (in `periodic_update`)

1. `s = (spread + spread_cv).clamp(0,1)`. For pairs `i=0..4`:
   `off_i = M[i]·s/24` octaves (`M = [0.18,0.43,0.71,1.00]`),
   `r = exp2(off_i)`; below side = `1/r`, above side = `r` (and inverses).
2. base pitch: `frequency` + `voct` + `fm` (via `fm_type`/`FMMode`) → Hz → cycles
   per sample → `base_inc` (Q32) and `inv_base = 2^32/inc`.
3. `inc[k] = base_inc · ratio[k]`, `inv_inc[k] = inv_base · inv_ratio[k]`
   (centre: ratio = 1).
4. density `D = (density + density_cv).clamp(0,1) · 4`; pair gain
   `g[i] = (D − i).clamp(0,1)`. Effective side count `eff = Σ 2·g[i]`.
   `w_side = mix_side / max(eff, ε)`, `w_centre = mix_centre`, where
   `(mix_centre, mix_side)` derive from `mix` (+ `mix_cv`), normalised.
5. Pack into the core's `gain[9]` (centre + 8 sides, each side scaled by its
   pair gain and `w_side`), call `core.update(&inc, &inv_inc, &gain)`.

FM/pitch are thus sampled at `periodic_update_interval` (control-rate vibrato) —
the deliberate consequence in ADR 0078 §6. Per-sample `process` does no
frequency maths.

## Acceptance criteria

- [x] `spread = 0` → unison: no detune beating; `spread = 1` beats.
      — `spread_zero_is_unison_spread_one_beats`.
- [x] `spread = 1` → outermost pair at ±50 cents (ratio 2^±1/24), verified on
      the packed core increments. — `spread_one_outermost_is_fifty_cents`.
- [x] Sweeping `density` 0→1 holds level (loudness-normalised) and never clips;
      `mix = 0` is the clean centre saw. — `density_sweep_holds_level_and_stays_bounded`,
      `mix_zero_is_clean_centre_saw`.
- [x] `fm` produces control-rate vibrato; `fm_type` logarithmic = ±octave per
      volt. — `fm_logarithmic_shifts_pitch_one_octave`.
- [x] Module doc comment in standard form (Inputs/Outputs/Parameters; port
      names match descriptor), notes control-rate FM + no sync/phase-mod.
- [x] Registered in `osc::mod` re-exports + the registry; instantiable from the
      DSL. — `default_registry_contains_all_modules`.
- [x] `just commit -p patches-modules` green; `cargo clippy` clean. — 481 tests.

## Result

Module: [`patches-modules/src/osc/hypersaw.rs`](../../patches-modules/src/osc/hypersaw.rs).
All control-rate maths (spread→ratios, pitch/FM→base increment, density→pair
gains, mix→balance) is in `recompute`, called from `periodic_update` (and seeded
from `set_ports`/`update_validated_parameters`). Per-sample `process` only runs
`core.process` and reads lane 0.

Two design choices worth noting:

- **Mix is normalised by `1 + mix`**: `mix = 0` → centre saw at gain 1 (clean);
  `mix = 1` → centre + sides each at half. `Σ|gain| ≤ 1`, so the summed output
  stays in `[-1, 1]` without a limiter. Full stack *includes* the centre.
- **Sides normalised by the effective active count** `Σ 2·g[i]`, so total side
  loudness holds constant as density fades pairs in.

Added `HyperSawCore::increment(copy, voice)` (read-only) so the module test can
verify the packed detune ratios.

## Notes

- `spread_cv`/`density_cv`/`mix_cv` are mono and shared (ADR 0078): preserves the
  8-shared-ratio factoring. Poly spread CV is explicitly deferred.
- Compute the 8 ratios once per period; do **not** recompute per copy.
- Keep `M`, `FULL = 1/24`, and pair-fade constants as documented `const`s with a
  one-line ADR 0078 reference.
