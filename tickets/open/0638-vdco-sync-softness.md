---
id: "0638"
title: VDco / VPolyDco — sync softness (RC-discharge model)
priority: medium
created: 2026-04-22
epic: "E103"
adr: "0047"
depends_on: ["0635"]
---

## Summary

Add a `sync_softness` parameter to `VDco` and `VPolyDco` that emulates
the non-instantaneous phase reset of analog hard sync (Jupiter-8
style). Instead of snapping phase to `0` on a sync event, the
accumulator's *target* snaps and the actual phase slews toward it
with a 1-pole exponential, matching the RC-discharge behaviour of the
sync cap.

## Acceptance criteria

- [ ] New parameter on `VDco` and `VPolyDco`:
      | Name            | Type  | Range    | Default | Description |
      |-----------------|-------|----------|---------|-------------|
      | `sync_softness` | float | 0.0..1.0 | `0.0`   | 0 = instant (pure hard sync, BLEP path from 0635). >0 blends toward an exponential phase slew with time constant τ(softness). |
- [ ] Mapping: `softness -> τ_samples`. Suggested curve:
      `τ = softness.powi(2) * 3.0` (so `0.5` ≈ 0.75 samples, `1.0` ≈ 3
      samples). Document choice in code comment.
- [ ] Per-voice state gains `phase_target: f32`. On sync event at
      `frac`: compute pre-reset waveform values as in 0635, set
      `phase_target = 0.0`. Leave `phase` alone (slew closes the gap).
      `sub_flipflop` still resets on event (discrete state, nothing
      to slew).
- [ ] Each sample: `phase += (phase_target - phase) * (1.0 - a)`
      where `a = exp(-1.0 / τ)` (with τ=0 → `a=0` → behaves like the
      existing hard reset path). Then normal `phase_increment`
      advance; `phase_target` advances with `phase_increment` too so
      target tracks the free-running phase once the gap closes.
- [ ] When `softness > 0`, skip the PolyBLEP residual from 0635 —
      the slew is already C⁰-continuous and adding BLEP would
      double-bandlimit. When `softness == 0`, take the 0635 BLEP path
      exactly.
- [ ] Unit tests: softness=0 matches 0635 output sample-for-sample;
      softness=0.5 shows measurable phase continuity across sync;
      softness=1 audibly (and numerically) rolls off the sync edge;
      `reset_out` still fires at the correct frac regardless of
      softness.

## Notes

Conceptually, this is a reduced-order model of the sync discharge RC.
τ ≤ 3 samples keeps the behaviour clearly "sync" rather than "no
sync". Users wanting a thicker effect can also stack a `PhaseMod`
or detune.

Interaction with sub square: `sub_flipflop` is discrete, so the sub
edge is always sharp even under softness. Accept this — matches
analog, where the /2 divider is a flip-flop clocked by the synced
waveform's edge.
