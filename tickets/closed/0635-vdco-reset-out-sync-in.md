---
id: "0635"
title: VDco / VPolyDco — reset_out and sync in, sub-sample BLEP
priority: high
created: 2026-04-22
epic: "E103"
adr: "0047"
depends_on: ["0632", "0633"]
---

## Summary

Add `reset_out` (`Trigger` / `PolyTrigger`) and `sync` in ports to
`VDco` and `VPolyDco`. Emit the fractional position of each phase
wrap on `reset_out`. On `sync` events, reset phase with PolyBLEP
applied at offset `1 - frac` for saw, pulse, and sub, scaled by each
waveform's pre→post jump.

## Acceptance criteria

- [ ] `patches-vintage/src/vdco/core.rs` exposes a wrap-frac return
      from `advance` (or a sibling helper) so the module layer can
      emit on `reset_out`.
- [ ] `sync` in handling: on event sample at `frac`, compute pre-reset
      waveform values at `phase + frac * dt` (wrapped), reset
      `phase = 0`, `sub_flipflop = false`, advance to
      `(1 - frac) * dt`, compute post-reset values, apply PolyBLEP
      residual at offset `1 - frac` scaled by `Δ = pre - post` for
      each active waveform.
- [ ] Pulse under sync: treat the sync jump as a single combined Δ
      per waveform, not separately at the comparator boundary.
- [ ] `reset_out` and `sync` ports wired for both `VDco` (mono-phase
      source, single-voice sync) and `VPolyDco` (per-voice, matching
      kinds on both sides).
- [ ] Unit tests covering: reset_out frac correctness for a range of
      increments; sync with `frac = 0.0`, `0.5`, `0.999`; sync with
      each subset of waveforms enabled; pulse comparator interaction.
- [ ] Aliasing spot-check (numeric): FFT of a hard-sync output shows
      noise floor below the equivalent threshold-synced baseline over
      the 5–20 kHz band at common sync ratios (e.g. 3:2, golden).
      Quantitative threshold left to reviewer judgement; include the
      measured numbers in a test comment.

## Notes

Preserve the existing invariant that the pulse comparator reads raw
phase, never BLEP-corrected saw (`core.rs` comment). Sync reset reads
the same raw phase.
