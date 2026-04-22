---
id: "0640"
title: VDco — subtle analog phasor curvature
priority: low
created: 2026-04-22
---

## Summary

Add a subtle, always-on nonlinearity to `VDco` / `VPolyDco`'s phase
ramp, modelling the non-constant charging current of an analog
integrator (finite output impedance, slight voltage-dependence of
the timing cap). The saw ramp becomes concave-down — rises fast
early, flattens near the top — which is part of the characteristic
warmth of analog DCOs/VCOs vs a perfectly linear digital phasor.

Dramatic CZ-style phase distortion is explicitly **out of scope** for
this ticket. That belongs in a future dedicated phase-distortion
module with its own DSL surface.

## Acceptance criteria

- [ ] New float parameter `phasor_curvature ∈ [0.0, 1.0]` on `VDco`
      and `VPolyDco`. Default `0.1` (subtle, audible as vintage
      colour but not exaggerated).
- [ ] Applied in `render_and_advance` to the phase value *read* for
      waveform generation. The accumulator itself keeps running on
      the linear phase; otherwise apparent frequency would shift.
      Approximation: `shaped = phase - curvature * phase * (1.0 - phase)`.
      Two muls, one add, one sub per sample. Quadratic approx of the
      RC cap-charge curve `(1 - exp(-k*phase)) / (1 - exp(-k))` for
      small `k`; indistinguishable at the curvature range we use.
      Endpoints pinned: `0 → 0`, `1 → 1`.
- [ ] All phase-derived waveforms (saw, pulse comparator, triangle
      wavefold from 0639, sub) read the shaped phase. Preserves the
      single-phasor phase-lock invariant.
- [ ] `reset_out` fires on linear-phase wraps (unchanged): the
      output cycle aligns with the true fundamental, not the warped
      phase.
- [ ] PolyBLEP uses the effective local phase increment
      `dt_eff = dt * (1.0 - curvature * (1.0 - 2.0 * phase))` — the
      derivative of the shape function. Cheap; no extra state.
- [ ] `phasor_curvature = 0.0` matches current linear behaviour
      sample-for-sample (regression test).
- [ ] Spectrum test: at `curvature = 0.1`, saw shows small but
      measurable harmonic-level deviation from linear baseline;
      shape is stable; no aliasing spikes introduced.

## Notes

This is always-on vintage character rather than a performance
parameter — set it once per patch. Sits alongside the existing PRNG
drift in `core.rs` as "baseline analog flavour" rather than a
performance control.

Interaction with 0638 (sync softness): curvature applies to the
phase read, softness affects how phase approaches its reset target.
They compose cleanly — the slew runs on linear phase; curvature
shapes the ramp the listener hears.

Interaction with 0639 (triangle via wavefold): triangle reads the
shaped phase, so its corners sit slightly off `0.5` under non-zero
curvature. Intentional — part of the analog character.

Out of scope: dramatic phase distortion (CZ-style piecewise remaps,
reflection, strong asymmetry). Defer to a future explicit
phase-distortion module.
