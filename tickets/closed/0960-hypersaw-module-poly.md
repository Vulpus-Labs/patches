---
id: "0960"
title: PolyHyperSaw — 16-voice detuned-saw oscillator module (patches-modules)
priority: medium
created: 2026-05-27
depends_on: ["0959"]
---

## Summary

Add the `PolyHyperSaw` module: the 16-voice counterpart of `HyperSaw` (ticket
0959), wrapping the **same single voice-batched `HyperSawCore`** but driving all
16 lanes. `voct` and `fm` are poly (per-voice); `spread`/`density`/`mix` CV stay
**mono/shared** so the 8 detune ratios are computed once and reused across all
voices (ADR 0078 §3; poly spread deferred). `HyperSaw` and `PolyHyperSaw` differ
only in how many lanes they fill and their port widths — the kernel is identical.

## Module surface

```text
name: "PolyHyperSaw", axes: [CHANNELS]
inputs:  voct (poly), fm (poly), spread_cv (mono), density_cv (mono), mix_cv (mono)
outputs: out (poly)
params:  same as HyperSaw (frequency, fm_type, spread, density, mix)
```

Use `PolyFrequencyConverter`/`PolyFrequencyChangeTracker` and `PolyInput`/
`PolyOutput`/`PolyTrigger`-free surface (no sync). No `phase_mod`, no
`reset_out`, no drift v1.

## Control-rate maths (in `periodic_update`)

- Compute the **8 ratios + inverses once** from the shared `spread`(+cv) and the
  shared `density`/`mix` gains — identical to 0959 but hoisted out of the voice
  loop.
- Per voice `v` (0..`poly_voices`): base pitch from `frequency` + `voct[v]` +
  `fm[v]` → `base_inc[v]`/`inv_base[v]`; fill column `v` of the core's
  `inc`/`inv_inc`/`gain` `[copy][voice]` arrays (sides scaled by the shared pair
  gains + `w_side`). One `core.update(...)` with the full batch.

Per-sample `process`: one `core.process(&mut out)` fills all 16 lanes; copy the
active voices to the poly output.

## Acceptance criteria

- [x] Each voice independently produces the detuned ensemble driven by its
      `voct[v]`/`fm[v]`; copy×voice phases decorrelated at construction (single
      `HyperSawCore::new(seed)`). — `voices_track_independent_voct`.
- [x] Shared `spread`/`density`/`mix` CV affect all voices identically; the 8
      ratios are computed once per period (`compute_detune` hoisted out of the
      voice loop), verified column-to-column. — `shared_cv_affects_all_voices`.
- [x] Output matches `HyperSaw` (0959) for a single driven voice — same pitch
      and long-run RMS (seeds differ, so parity is spectral/level, not
      sample-exact). — `parity_with_mono_for_single_voice`.
- [x] No allocation on the audio thread; no `unwrap`/`expect`; clippy clean. —
      `process` is `core.process` + `write_poly`; recompute is control-rate.
- [x] Module doc comment in standard form; registered + re-exported;
      instantiable from the DSL. — `default_registry_contains_all_modules`.
- [x] `just commit -p patches-modules` green. — 486 tests, clippy clean.

## Result

Module: [`patches-modules/src/osc/poly_hypersaw.rs`](../../patches-modules/src/osc/poly_hypersaw.rs).
The voice-shared detune/density/mix maths was extracted from 0959 into
`compute_detune` + `pack_voice` + `base_increment` (in
[`osc/hypersaw.rs`](../../patches-modules/src/osc/hypersaw.rs)); `HyperSaw` calls
them for lane 0, `PolyHyperSaw` computes `compute_detune` once then loops 16
voices filling each column. Same kernel, same per-copy maths — the modules
differ only in lane count and port widths, as the ticket required.

## Notes

- Seed the core from `instance_id` so the copy×voice phase pattern differs per
  instance.
- The kernel and its vectorisation are 0958's responsibility (voice-batched,
  copy-major, ASM-gated). This ticket just wires 16-lane poly I/O to it and does
  the per-voice control-rate fill. Add no SIMD or dependency here.
- If the 0958 ASM gate failed and forced the explicit-SIMD fallback, that lands
  in 0958, not here — this module is agnostic to how the kernel vectorises.
