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

## Design

### Invariants

- **Audio side is dumb.** Tap module reads inputs, writes lanes into
  backplane (32 lanes × 64 samples), zeroes unused lanes. No slot ids,
  no tagging, no manifest awareness. Lane index = position, full stop.
- **Lane → processor mapping is observer-only.** Audio thread never
  knows what a lane "means". Replan touches observer state only.
- **Frame layout on the wire: sample-major** (`[[f32; MAX_TAPS];
  TAP_BLOCK]`) for producer cache-friendliness. **Observer
  transposes** on receipt into lane-major work buffers
  (`[[f32; TAP_BLOCK]; MAX_TAPS]`) so each processor sees a
  contiguous `&[f32; TAP_BLOCK]` (SIMD-friendly). Transpose is a
  one-shot 32×64 reshape per block, off the audio thread.

### Processor model

Processors are stateless-to-test: pure function of `(lane samples,
internal state) → 0+ Observations`.

```rust
enum Observation {
    Level(f32),           // meter peak, meter rms, gate, trigger
    Spectrum([f32; N]),   // FFT bin magnitudes
    Scope(Box<[f32]>),    // oscilloscope buffer
}

trait Processor {
    fn process(&mut self, lane: &[f32; 64]) -> SmallVec<[Observation; 2]>;
}
```

Caller (the observer thread loop) combines observation + sample
timestamp + processor id (`"filter.rms"`, `"filter.peak"`, etc.) into
UIEvents shipped to the UI thread. Processor itself doesn't know its
id or timestamp.

### Processor identity & replan

Identity key: `(tap_name, processor_type, params)`.

```text
on new manifest:
  for desc in new_manifest:
    key = (desc.tap_name, desc.kind, desc.params)
    if old.remove(&key) is Some(p) -> reuse
    else -> build fresh
  drop remainder
```

Param change (e.g. `meter.window` ms) → different key → rebuild with
new allocation. Acceptable: we are not fussy about state continuity.

### Slots & subscriber surface

`slots: Vec<Vec<Box<dyn Processor>>>` indexed by lane 0..32. Each lane
may have multiple processors (e.g. `meter` = peak + rms = 2
processors, or 1 processor emitting 2 observations — implementation
choice).

Subscriber surface holds the latest shipped UIEvent values per
`(lane, processor_id)` for atomic-scalar consumption per ADR 0053 §7,
plus a small SPSC ringbuf for diagnostics ("not yet implemented",
future variants) and forwarded drop counters from 0700.

### Replan transport

Control thread → observer thread: SPSC `Arc<Manifest>` channel.
Observer drains on each iteration before reading frames. Sample rate
travels with the manifest (or in a startup config; revisit if rate
change becomes a thing).

### Pipeline coverage (this ticket)

- `meter`: peak (running max-abs + ballistic decay) + rms (rolling
  window sized by `window_ms × sample_rate`). Two observations per
  process call.
- `osc`, `spectrum`, `gate_led`, `trigger_led`, compound-with-any-of-
  these: stub processor, no observations, one-shot diagnostic on
  manifest receipt.

### Tests

- Synthetic lane stream → processor → expected peak/rms trajectory.
- Sample-rate awareness: same params at 44.1k vs 48k vs 96k →
  consistent time-domain behaviour.
- Replan: identity-key reuse vs rebuild on param change.
- Unbound lanes (zeros) → observations are 0.

## Cross-references

- ADR 0053 §§5–7 — ring, latest-scalar surface.
- ADR 0054 §§6–7 — manifest, pipelines.
- ADR 0056 — observer pipeline and frame layout (this design).
- E119 — parent epic.
- 0700 — frame ring (producer side). Closed but ships sample-major
  per-tick frames; **0706 retrofits to lane-major block frames +
  `sample_time`**. 0701 lands after 0706.
