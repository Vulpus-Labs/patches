# ADR 0056 — Observer pipeline and tap frame layout

**Date:** 2026-04-26
**Status:** Accepted
**Related:**
[ADR 0043 — Cable tap observation](0043-cable-tap-observation.md),
[ADR 0053 — Observation three-thread split](0053-observation-three-thread-split.md),
[ADR 0054 — Tap DSL and modules](0054-tap-dsl-and-modules.md),
[ADR 0055 — Observation bringup via ratatui patches-player](0055-observation-bringup-via-ratatui-player.md)

## Context

ADR 0053 fixed the three-thread split (audio → ring → observer →
subscriber) and the latest-scalar subscriber surface. ADR 0054
specified the tap manifest. Ticket 0700 shipped a working SPSC frame
ring but settled some shape questions provisionally — per-sample
frames carrying `[f32; MAX_TAPS]`, no timestamp — explicitly deferring
the observer-side layout to a later decision. Ticket 0701
(patches-observation crate) is now imminent, which forces those
decisions.

This ADR captures the design choices made in the planning session for
0701 + 0706, so the following implementation tickets land against a
documented contract instead of inferred constraints.

The decisions cover:

1. The audio/observer interface contract.
2. Frame layout on the wire (and why it differs from the layout
   processors prefer).
3. The observer-side processor model.
4. Replan and processor identity.
5. Sample timing and UI sync.
6. Scope-style processors with parameter feedback.

## Decisions

### 1. Audio side stays dumb

The audio thread is a pure backplane producer. Tap modules read their
inputs, write into the lanes they're bound to, and zero unused lanes.
There is no slot id, no tag, no manifest awareness, no per-tap metadata
on the audio path. Lane index is implicit by position in the
`[f32; MAX_TAPS]` backplane.

The mapping from lane → processor lives entirely in the observer.
Replanning the observer (different processors, different parameters,
different tap set) does not require any audio-thread coordination
beyond the existing patch-builder lane wiring.

This keeps the audio path's hot loop free of dispatch logic and makes
the observer side independently re-buildable.

### 2. Sample-major block frames on the wire

The frame format on the SPSC ring is a **block-rate**, **sample-major**
struct:

```rust
pub const TAP_BLOCK: usize = 64;

pub struct TapBlockFrame {
    /// samples[i] = full backplane snapshot for sample i of this block.
    pub samples: [[f32; MAX_TAPS]; TAP_BLOCK],
    /// Monotonic sample index of samples[0]. Resets on engine rebuild.
    pub sample_time: u64,
}
```

The producer writes one row (`samples[idx] = backplane`, a contiguous
128 B memcpy = 2 cache lines) per audio tick, increments `idx`, and
pushes the block when `idx == TAP_BLOCK`. This preserves the
cache-friendly per-sample write pattern of the current
`try_push_frame(&[f32; MAX_TAPS])` design while cutting ring overhead
~64×.

Lane-major-on-the-wire (`[[f32; TAP_BLOCK]; MAX_TAPS]`) was rejected:
it would force the producer to scatter writes across 32 distinct
cache lines per sample (each lane is a 256 B buffer), which is hostile
to the audio thread.

### 3. Observer transposes once on receipt

Observer-side processors want lane-major contiguous slices
(`&[f32; TAP_BLOCK]` per lane) for SIMD reductions (peak, RMS, FFT
input, scope buffer). The observer thread transposes each frame on
receipt — a one-shot 32×64 reshape per block, off the audio thread —
into lane-major work buffers, then dispatches lanes to processors.

This pays the transpose cost in the place that can afford it.

### 4. Processor model: stateless interface, observation outputs

Processors are designed for cheap, stateless testing: pure of any
threading or transport concern.

```rust
enum Observation {
    Level(f32),                 // meter peak, meter rms, gate, trigger
    Spectrum(Box<[f32]>),       // FFT bin magnitudes
    Scope(Box<[f32]>),          // oscilloscope buffer
}

trait Processor {
    fn process(&mut self, lane: &[f32; TAP_BLOCK])
        -> SmallVec<[Observation; 2]>;
}
```

The observer loop wraps each `Observation` with the processor id
(`"filter.peak"`, `"filter.rms"`, …) and the frame's `sample_time`,
producing UIEvents shipped via the subscriber surface. The processor
itself does not know its id or current sample time; it does not know
what lane it is bound to. This is what makes processors trivially
unit-testable.

A single tap component may produce multiple processors (e.g. `meter`
= peak processor + rms processor) or a single processor emitting
multiple observations per call. Implementations are free to pick.

### 5. Processor identity & replan

Processor identity key is `(tap_name, processor_type, params)`. On
manifest replan:

```text
for desc in new_manifest:
    key = (desc.tap_name, desc.kind, desc.params)
    if old.remove(&key) is Some(p): reuse
    else: build fresh
drop the remainder of old
```

Param changes (e.g. `meter.window_ms`, scope `time_per_div`) produce a
different key and trigger rebuild with fresh allocation. The system
is **not fussy about state continuity** across replan — RMS history,
scope ring contents, etc. are dropped. This is a deliberate
simplification: the alternative (stateful param updates over a
separate UI-to-observer channel) was rejected as architectural
complexity not justified by the user impact.

UI parameter changes therefore ride the same manifest-reissue channel
as topology changes. There is no dedicated UI→observer parameter path.

### 6. Subscriber surface

Per ADR 0053 §7, subscribers read latest values from atomic scalars
per `(lane, processor_id)`. Diagnostics ("not yet implemented" for
unsupported components, future variants) and forwarded drop counters
share a small SPSC ringbuf alongside the scalar surface. Adding a new
diagnostic kind = adding a variant, not a new channel.

Spectrum and scope outputs (non-scalar) need their own publish
mechanism (per-slot double buffer or seqlock) parallel to the scalar
surface. Out of scope for the meter-only bringup; the surface module
must leave room for it.

### 7. Sample timing & UI sync

The `sample_time` field on `TapBlockFrame` is the monotonic sample
index of the block's first sample, sourced from a fresh `u64` counter
on `PatchProcessor` (separate from the existing `sample_count` which
wraps at 2^16 for transport). The counter resets on engine rebuild;
sample-rate change is only possible via engine rebuild, so sample
time is unambiguous within a single engine lifetime.

UIEvents carry `sample_time` directly. The UI converts via the known
sample rate for cross-event alignment (scope sweep start, multi-lane
sync). The UI **must not** mix wall-clock time with sample time for
sync — wall-clock is for paint scheduling only.

`u64` wraparound at 96 kHz is ~6 million years; non-issue.

### 8. Oscilloscope: pre-process in observer, params via manifest

Two options for scope: passthrough lane data with UI-side decimation
+ trigger, or observer-side trigger/decimation/window with UI as pure
renderer. We pick observer-side: passthrough would ship ~750
blocks/sec/lane, most discarded by the UI's display rate. Observer
already has the samples and the timing data.

Scope params (`window_samples`, `decimation`, `trigger_level`,
`trigger_edge`, `update_rate`) live in the manifest. UI tweaks → host
reissues manifest → observer's identity-key replan rebuilds the scope
processor with new params. No separate UI→observer param channel
(consistent with §5). Trade-off: scope buffer/history is dropped on
every param tweak. Acceptable now; revisit only if real usage shows
the loss is annoying.

## Consequences

- Frame format is fixed for 0706 (TAP_BLOCK = 64, sample-major,
  `sample_time`). Any future change here is breaking for both the
  producer and the observer crate.
- `patches-observation` (0701) can be implemented against a
  documented frame contract instead of an inferred one.
- Processors are unit-testable in isolation (no ring, no thread, no
  manifest); the observer crate's tests focus on dispatch + replan +
  surface, processors get their own focused tests.
- Replan is cheap on the observer thread but throws state — fine for
  current scope, may need revisiting if/when expensive-to-rebuild
  pipelines (large FFT plans, long impulse responses) land.
- No UI→observer parameter channel keeps the architecture small; if
  scope param-tweak history loss becomes a problem, the fix is local
  to scope (soft param update path) rather than a new global channel.
- Tap module's responsibility for zeroing unused lanes propagates
  through to "no-op processor" not being needed for empty lanes —
  a meter on a zero lane reports zero, which is correct.
