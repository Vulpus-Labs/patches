# ADR 0043 — Cable-tap observation

**Date:** 2026-04-17
**Status:** Proposed
**Supersedes:** [ADR 0021 — Observation event bus](0021-control-event-bus.md)
**Related:** [ADR 0015 — Polyphonic cables](0015-polyphonic-cables.md), [ADR 0016 — MIDI as the sole external control mechanism](0016-midi-only-control-architecture.md)

## Context

Non-real-time consumers — web UI level meters, oscilloscope displays,
external trigger/gate watchers, logging sinks — need a view into what
audio-thread signals look like. They never need to *influence* the
graph; they only need to read.

ADR 0021 proposed an opt-in `EmitsObservations` trait on modules plus a
typed `ObservationEvent` enum (Meter, PolyMeter, ScopeChunk). Each
module that wants to be observable implements the trait, computes its
own peak/RMS/etc., and pushes tagged events into a ring buffer.

That design puts observation at the wrong layer. Peak/RMS, oscilloscope
windowing, trigger detection, and gate-edge detection are all **pure
functions of a sample stream**. The source of the stream is a cable,
not a module: cables are what signals flow along, what the DSL names,
what users point at when they say "show me a meter on the filter
output". Binding observation to the emitting module forces every
observable module to re-derive the same statistics, and makes
observation impossible for cables whose producing module has not opted
in.

## Decision

### 1. Observation is an engine-level cable tap

The engine gains the ability to **tap a cable** — to copy that cable's
sample values into an outbound transfer buffer each tick. Taps are a
property of the execution plan, not of modules. Any cable in the graph
is tappable, regardless of what module produces or consumes it.

Modules have no awareness of observation. No trait, no sink, no emit
method. `EmitsObservations` from ADR 0021 is dropped.

### 2. Tap payload: raw sample streams

A tap ships raw samples, not derived statistics. For a mono cable, one
`f32` per sample; for a poly cable, `[f32; 16]` per sample. The engine
does no arithmetic on tapped data beyond the copy.

All derivation — peak/RMS, oscilloscope windowing, zero-crossing
detection, gate/trigger edges, FFT bins, histograms — happens on the
observer thread. That thread is non-real-time and may allocate, block,
and use any data structure it wants. Shipping raw samples means the
set of derivable views is open-ended: a new analyser can be added
without touching the audio thread.

### 3. Transfer mechanism

A single shared ring buffer (`rtrb::RingBuffer`) carries tap payloads
from the audio thread to a `"patches-observer"` consumer thread,
following ADR 0021's threading model. Payloads are written once per
sub-block (every 64 samples) rather than per sample, amortising the
push cost. Each payload is tagged with the tap's `CableId` so the
consumer can route it.

Payload shape (sketch):

```rust
/// One sub-block's worth of tapped samples from a single cable.
#[derive(Clone, Copy)]
pub enum TapPayload {
    Mono { cable: CableId, samples: [f32; 64] },
    Poly { cable: CableId, samples: [[f32; 16]; 64] },
}
```

A poly payload is large (~4 KB). At realistic tap counts (tens, not
thousands) the ring buffer remains well under 1 MB/sub-block. Full
ring buffer = silent drop, same policy as ADR 0021: observation is
lossy by design.

### 4. Plan integration

`ExecutionPlan` gains a tap list: `taps: Vec<TapSpec>` where each
`TapSpec` names a cable index and its kind (mono/poly). The planner
populates this from an external attach registry (see §6 below — API
deferred). The audio thread walks the tap list after each sub-block
and pushes one `TapPayload` per tap into the ring buffer.

Taps are added/removed across plan rebuilds the same way cables are.
Nothing about tap wiring is special relative to graph wiring — it is
graph wiring, for a non-module consumer.

### 5. Consumer side

The observer thread drains the ring buffer and fans payloads out to
registered analysers keyed by `CableId`:

- **Level meter**: maintains rolling peak/RMS per cable, flushes at 30 Hz.
- **Oscilloscope**: accumulates a trigger-aligned window, flushes at
  60 Hz.
- **Trigger/gate watcher**: runs edge detection, forwards events.
- **Logger / WebSocket**: serialises raw or derived values.

Multiple analysers can subscribe to the same tap; the observer thread
demuxes. Rate-limiting and serialisation policy stay on the observer
thread, as in ADR 0021 §7.

### 6. Attach API: deferred

How a tap gets registered — DSL syntax, LSP command, runtime-only API,
UI gesture, or some combination — is deliberately left open. The
design needs more thought, especially around tap lifetime across plan
rebuilds (does a tap survive if its cable is replaced?), addressing
(cable expressions? post-expansion names?), and permission
(self-serve from the LSP, or explicit patch-level declaration?).

Whatever the attachment surface, it must reduce to a `Vec<TapSpec>`
that the planner can include in the next plan.

## Consequences

**Positive**

- Observation is orthogonal to module implementation. Any cable is
  observable. New observable cables require no module changes.
- No duplication of statistics code across modules. Peak/RMS
  implemented once on the observer thread.
- New derived views (FFT, histogram, phase correlator) are pure
  observer-thread additions.
- The audio-thread contract stays simple: copy one sub-block per tap
  per tick; no per-module bookkeeping, no conditional branches in the
  tick loop beyond iterating the tap list.
- Poly cables are tappable with the same mechanism. Bus metering and
  transport snooping fall out for free.

**Negative**

- Bandwidth is higher than a tagged-event scheme. 10 mono taps at
  48 kHz = ~1.9 MB/s; 10 poly taps = ~30 MB/s. Manageable in absolute
  terms but worth watching. Per-sub-block aggregation and bounded tap
  counts keep it in check.
- Derivation latency: a meter reading lags by up to one sub-block
  (~1.3 ms at 48 kHz) plus observer-thread scheduling. Invisible in
  UI, noted for record.
- Attach API is a design debt. The engine-side mechanism is the easy
  part; naming cables from outside the audio thread is where the
  complexity sits.

**Neutral**

- Threading model from ADR 0021 §8 is unchanged. The observer thread
  stays; its input format changes from tagged events to raw payloads.
- Ring buffer sizing, drop policy, and cadence stay as in ADR 0021.

## Alternatives considered

### Retain ADR 0021's module-emit design

Rejected per the reasoning above: wrong layer, duplicated statistics,
forces opt-in on every observable module, closes off analysis kinds
not anticipated at module-author time.

### Derive statistics on the audio thread, ship derived values

A middle ground: engine computes peak/RMS/etc. per tap, ships only
derived values. Cheaper bandwidth, but locks in the set of derivable
views at engine-build time and puts arithmetic on the audio thread
for views that may not currently be consumed. Rejected: the
observer thread has cycles to spare, and keeping the audio thread's
contribution to pure copy is the point.

### Poly-frame backplane reuse

Already considered and rejected in ADR 0021's alternatives. The
semantic mismatch (last-write-wins vs stream) is the same here, so
the rejection carries over.

## Cross-references

- ADR 0015 — polyphonic cables (defines the per-cable payload shape).
- ADR 0016 — MIDI as the sole inbound control mechanism (unchanged;
  this ADR is outbound-only).
- ADR 0021 — superseded by this ADR.
