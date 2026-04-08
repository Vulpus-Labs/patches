---
id: "E031"
title: Audio-thread hot-path optimisations
status: closed
priority: medium
created: 2026-03-23
tickets:
  - "0179"
  - "0180"
  - "0181"
---

## Summary

Three performance improvements identified by profiling `poly_synth_layered.patches`
with samply (30 s, 1000 Hz sampler, Apple Silicon).  The patch currently runs with
~2× real-time headroom (52% of audio thread time is CoreAudio idle wait); these
fixes reduce unnecessary computation in `FdnReverb` and `CablePool` and will
extend that headroom further, particularly under heavier patches with multiple
reverb instances or more poly voices.

## Profile findings

| Finding | Self % (audio thread) | % of compute | Impact |
|---------|-----------------------|--------------|--------|
| `FdnReverb`: `derive_params` called every sample (one `powf`) | included in 5.5% FdnReverb | ~3–4% est. | medium |
| `FdnReverb`: `recompute_absorption` every 32 samples when static | `absorption_coeffs` visible in leaf data | ~2–3% est. | medium |
| `CablePool::read_poly`: 16-channel multiply even when scale=1.0 | **2.9% self** | 6.1% | medium |

## Tickets

- [T-0179](../tickets/open/0179-fdn-reverb-cache-derived-scale.md) — Cache `scale` in `FdnReverb`; eliminate per-sample `derive_params` call
- [T-0180](../tickets/open/0180-fdn-reverb-dirty-flag-absorption.md) — Dirty-flag `recompute_absorption`; skip when CV unconnected and params unchanged
- [T-0181](../tickets/open/0181-cable-pool-read-poly-scale-fast-path.md) — `CablePool::read_poly` scale=1.0 fast path

## Related

- [T-0178](../tickets/open/0178-periodic-update-trait.md) — `PeriodicUpdate` trait moves coefficient update out of `process()`.
  T-0180 adds a dirty-flag gate on top of the existing counter mechanism; T-0178
  refactors the counter itself into a trait method.  The two are independent and
  can land in either order.
