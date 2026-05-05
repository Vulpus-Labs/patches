---
id: E134
title: Denormal hardening across DSP and vintage modules
status: open
created: 2026-05-04
---

## Goal

Eliminate denormal-induced CPU spikes in the audio path. Subnormal
floats (< ~1.18e-38 for f32) take the microcode slow path, costing
10–100× per op. In audio this manifests as CPU rising *during silence*
(reverb tails, idle voices with open envelopes, feedback decays),
risking xruns on patches that profiled fine while playing.

Survey of `patches-dsp`, `patches-modules`, and `patches-vintage`
identified mitigated sites (svf/ladder use `sanitize()`; dc_blocker /
envelope_follower / adsr flush or snap) and unmitigated hotspots:

- vflanger / vflanger_stereo / vbbd / vstereobbd `fb_state` writes
- vreverb FDN damping cascade (`damp_z1`, `damp_z2`)
- bbd_filter_proto complex pole state
- patches-dsp biquad TDFII linear mode (no-saturate path)
- limiter_core smoothed gain envelope

## Approach

Two layers:

1. **FTZ/DAZ at the audio callback** (primary defense). Sets MXCSR
   bits on x86_64, FPCR.FZ on aarch64. Per-thread, set every
   callback (hosts may reset). Eliminates denormals globally for the
   entire DSP graph at the cost of one register write per buffer.

2. **Per-site flush in identified hotspots** (defense in depth).
   Covers offline render paths (WAV bounce, tests) that don't go
   through the audio callback, and protects against future hosts
   that reset MXCSR mid-callback.

`sanitize()` (NaN/Inf guard) stays — orthogonal concern.

## Tradeoffs of FTZ/DAZ

- **Per-thread, per-callback**: must set inside the callback, not
  once at startup. Worker threads in any future parallel
  `ExecutionPlan::tick()` need their own setup.
- **Not IEEE-strict**: subnormal values flush to zero. Inaudible
  (~-700 dBFS) but breaks bit-exact reproducibility of offline
  renders across CPUs without FTZ. Determinism tests must be audited.
- **aarch64 has no separate DAZ**: FPCR.FZ flushes both inputs and
  outputs. No fine-grained control. Acceptable for our use.
- **Doesn't help non-audio threads**: offline render / tests need
  per-site flush or explicit FTZ guard.
- **Affects all FP ops on the thread**, not just DSP. Rare to matter
  in our callback (no non-DSP work there) but worth noting.

## Tickets

- 0802 — FTZ/DAZ in audio callback (engine + clap host)
- 0803 — Audit determinism tests for FTZ sensitivity
- 0804 — Sanitize vintage feedback state (vflanger, vbbd, vreverb)
- 0805 — Flush biquad TDFII linear mode + limiter_core gain envelope
- 0806 — Denormal CPU-cost regression test

## Done when

- Audio callback sets FTZ/DAZ on entry on x86_64 and aarch64.
- Identified hotspots in patches-dsp and patches-vintage have
  per-site denormal mitigation.
- Regression test demonstrates flat CPU cost as a reverb tail
  decays into silence (was: rising; expected: flat).
- Existing determinism tests still pass, or are documented as
  FTZ-dependent.
