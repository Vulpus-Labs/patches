---
id: "0701"
title: patches-observation crate — consumer, meter pipeline, subscriber surface
priority: high
created: 2026-04-26
---

## Summary

Create a new `patches-observation` crate that owns the observer-side
runtime: consumer end of the SPSC ring (ticket 0700), per-slot
pipeline state, and a subscriber surface. Initial pipeline: `meter`
(fused running max-abs with ballistic decay + rolling-window RMS, per
ADR 0054 §7). Sample-rate aware. Other tap types stub out with a
"not yet implemented" path so unsupported declarations don't silently
no-op.

## Acceptance criteria

- [ ] New crate `patches-observation` with no engine, audio, or UI
  dependencies. Depends only on `patches-core`, `patches-dsl`
  (manifest type), and lock-free primitives.
- [ ] Owns the consumer end of the frame ring; runs its own thread
  with a controllable shutdown.
- [ ] Manifest-driven pipeline allocation: per `TapDescriptor`, build
  pipeline state on the observer thread (RMS window buffer, peak
  decay state). Observer never allocates on the audio thread's
  behalf.
- [ ] `meter` pipeline implements:
  - rolling-window RMS sized by `meter.window` (ms × sample_rate)
  - running max-abs with ballistic decay configured by `meter.decay`
    (defaults documented; both surfaced to subscribers)
- [ ] Other components (`osc`, `spectrum`, `gate_led`, `trigger_led`)
  build no-op pipelines that keep last-sample only and emit a
  one-shot "not yet implemented" diagnostic to the event log on
  manifest receipt. Compound taps containing any unimplemented
  component land here too.
- [ ] Subscriber surface: atomic-scalar "latest values" per slot per
  ADR 0053 §7. Subscribers read without locking. API exposes both
  per-slot peak and per-slot RMS for `meter`; one slot, two values.
- [ ] Per-tap drop counters readable from this crate (forwarded from
  ticket 0700's atomic counters keyed by slot).
- [ ] Tests: synthetic frame stream → observer → expected peak/RMS
  trajectory. Sample-rate awareness covered (same params, different
  rates, consistent output in time-domain).

## Notes

Manifest type currently lives in `patches-dsl`; this crate consumes
it as-is. Phase 3 may relocate it to `patches-core` or a shared
`patches-observation-types` crate; do not move it as part of this
ticket.

The "not yet implemented" diagnostic surfaces through the same
subscriber channel as drop counters — keep the diagnostic surface
shaped so future pipelines slot in by adding a variant rather than a
new channel.

## Cross-references

- ADR 0053 §§5–7 — ring, latest-scalar surface.
- ADR 0054 §§6–7 — manifest, pipelines.
- E119 — parent epic.
