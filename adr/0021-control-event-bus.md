# ADR 0021 — Observation event bus

**Date:** 2026-03-29
**Status:** Proposed
**Related:** [ADR 0016 — MIDI as the sole external control mechanism](0016-midi-only-control-architecture.md)

## Context

Modules producing peak/RMS levels, LED intensities, or oscilloscope waveform
chunks need a way to push data to a non-real-time consumer (web UI, monitoring
tools) without blocking the audio thread. There is currently no outbound data
path from the audio thread other than the audio output itself.

### Inbound control is already solved

ADR 0016 established MIDI as the sole external control mechanism. This
remains the correct approach for inbound control: DAWs, hardware controllers,
and web UIs can all send MIDI CC messages through the existing MIDI pipeline
(E021). A web UI sending control changes does so by generating MIDI CC over a
WebSocket-to-MIDI bridge, which feeds into the same `MonoMidiIn` → signal
path that hardware controllers use. No additional inbound control mechanism
is needed.

### What is needed

An outbound observation bus: a lock-free path for modules to push metering,
scope data, and other introspection information to non-real-time consumers.
This must respect the audio-thread constraints established in the project's
conventions: no allocations, no blocking, no I/O.

## Decision

### 1. Observation event type

Outbound events use a fat enum sized to the largest variant:

```rust
/// An observation event emitted by a module on the audio thread.
///
/// All variants are fixed-size and `Copy`. The enum is sized to the largest
/// variant (`ScopeChunk`); smaller variants waste space but avoid allocation.
#[derive(Clone, Copy)]
pub enum ObservationEvent {
    /// A single-channel level reading (peak + RMS).
    Meter {
        instance: InstanceId,
        channel: u8,
        peak: f32,
        rms: f32,
    },
    /// A polyphonic level reading (per-voice peaks).
    PolyMeter {
        instance: InstanceId,
        channel: u8,
        peaks: [f32; 16],
    },
    /// A chunk of mono audio samples for oscilloscope display.
    ///
    /// The effective sample rate (base rate × oversampling factor) is fixed
    /// for the lifetime of the engine and communicated to consumers at
    /// connection time, not per chunk. A 64-sample chunk at 4× oversampling
    /// spans ~0.33 ms rather than ~1.33 ms; the consumer must account for
    /// this when rendering.
    ScopeChunk {
        instance: InstanceId,
        channel: u8,
        samples: [f32; 64],
    },
}
```

`size_of::<ObservationEvent>()` is dominated by `ScopeChunk` (~268 bytes).
`Meter` variants occupy the same footprint. At realistic emission rates
(~1500 events/sec for a few emitting modules), throughput through the ring
buffer is well under 1 MB/sec.

### 2. Ring buffer

A single `rtrb::RingBuffer<ObservationEvent>` connects the audio thread
(producer) to a consumer thread. The ring buffer is pre-allocated at engine
start with a fixed capacity (e.g. 1024 slots, ~268 KB).

If the ring buffer is full, events are silently dropped. Observation data is
ephemeral — missing a meter update or scope chunk has no audible consequence.
The consumer thread is responsible for rate-limiting delivery to the UI
(e.g. sending metering updates at 30 Hz regardless of emission rate).

### 3. `EmitsObservations` trait

```rust
/// Opt-in trait for modules that emit observation events.
///
/// Called by the audio thread at sub-block boundaries. The module pushes
/// events into the provided `ObservationSink`, which writes to the shared
/// ring buffer. The module does not hold a reference to the ring buffer.
pub trait EmitsObservations {
    /// Emit observation events into the sink.
    ///
    /// Called once per sub-block (every 64 samples). The sink is a borrowed
    /// handle — do not store it.
    ///
    /// **Must not allocate, block, or perform I/O.**
    fn emit(&mut self, sink: &mut ObservationSink);
}
```

`Module` gains a default method:

```rust
fn as_observation_emitter(&mut self) -> Option<&mut dyn EmitsObservations> {
    None
}
```

This follows the established opt-in pattern (`as_midi_receiver`,
`as_periodic`). Modules that do not emit observations pay zero cost.

### 4. `ObservationSink`

A short-lived wrapper around the ring buffer producer, passed by the audio
thread during the emit call:

```rust
/// A borrowed handle to the outbound observation ring buffer.
///
/// Passed to modules during `EmitsObservations::emit`. Tags each event
/// with the module's `InstanceId` so the consumer can identify the source.
/// Silently drops events when the ring buffer is full.
pub struct ObservationSink<'a> {
    tx: &'a mut rtrb::Producer<ObservationEvent>,
    instance: InstanceId,
}

impl<'a> ObservationSink<'a> {
    pub fn meter(&mut self, channel: u8, peak: f32, rms: f32) {
        let _ = self.tx.push(ObservationEvent::Meter {
            instance: self.instance,
            channel,
            peak,
            rms,
        });
    }

    pub fn scope_chunk(&mut self, channel: u8, samples: &[f32; 64]) {
        let _ = self.tx.push(ObservationEvent::ScopeChunk {
            instance: self.instance,
            channel,
            samples: *samples,
        });
    }

    pub fn poly_meter(&mut self, channel: u8, peaks: &[f32; 16]) {
        let _ = self.tx.push(ObservationEvent::PolyMeter {
            instance: self.instance,
            channel,
            peaks: *peaks,
        });
    }
}
```

This follows the `CablePool` pattern: a borrowed handle to shared
infrastructure, passed for the duration of a call, not stored by the module.

### 5. Dispatch

`ExecutionState` gains an `emit_observations` method called at each sub-block
boundary, after `tick`:

```rust
pub fn emit_observations(
    &mut self,
    pool: &mut ModulePool,
    observation_tx: &mut rtrb::Producer<ObservationEvent>,
) {
    // Iterate pre-resolved emitter indices (built during rebuild)
    // For each, create an ObservationSink and call emit()
}
```

`ExecutionPlan` gains an `observation_emitter_indices: Vec<usize>` field,
populated by the planner from modules returning `Some` from
`as_observation_emitter()`.

### 6. Audio callback integration

The sub-block boundary in `fill_buffer` gains one additional call:

```
while remaining > 0 {
    if at sub-block boundary {
        dispatch_midi_events(...)      // existing
    }

    process_chunk(...)

    if at sub-block boundary {
        emit_observations(...)         // new
    }
}
```

Observation emission happens *after* processing so that emitted values
reflect the most recent outputs.

### 7. Consumer thread

A dedicated `"patches-observer"` thread drains the observation ring buffer
and fans events out to registered consumers (WebSocket connections, logging
sinks, etc.). This thread:

- Wakes on a timer (e.g. every 16 ms / ~60 Hz) or on ring-buffer readiness.
- Drains all available events.
- Rate-limits per consumer: a web UI receiving meter updates at 30 Hz does
  not need every event; the thread keeps only the latest value per
  `(instance, channel)` and flushes at the consumer's cadence.
- Is not real-time: may allocate, do I/O, hold mutexes. It runs entirely
  outside the audio-thread boundary.

### 8. Threading model (updated)

| Thread | Purpose | Communicates via |
|--------|---------|------------------|
| **Audio** | Sample processing, plan adoption, event dispatch | Ring buffers (rtrb) |
| **MIDI connector** | Opens ports, timestamps MIDI events | seqlock (AudioClock) + ring buffer |
| **Observer** | Drains observation events, fans out to consumers | ring buffer (observation) |
| **Cleanup** | Off-thread deallocation | ring buffer (cleanup actions) |

## Consequences

**ADR 0016 remains valid and is reinforced.** MIDI is the sole inbound
control mechanism. External control surfaces (including web UIs) send MIDI CC
through the existing pipeline. The observation bus is outbound-only and does
not interact with the MIDI path.

**No new allocation or blocking on the audio thread.** The ring buffer is
pre-allocated. The `ObservationSink` is stack-allocated and borrowed. Full
ring buffers cause silent drops, never blocking.

**Observation events are lossy and best-effort.** The audio thread never
blocks waiting for the consumer. Missing events have no audible or functional
consequence. The consumer thread handles rate-limiting and delivery
scheduling.

**The fat enum wastes space for small events.** A `Meter` event occupies
~268 bytes when it needs ~12. If this becomes a concern (e.g. when adding a
large variant like FFT bins), the single ring buffer can be split into
per-type ring buffers behind the `ObservationSink` abstraction without
changing the module-facing API.

## Alternatives considered

### Inbound control event bus (parallel to MIDI)

A general-purpose `(event_id, f32)` inbound bus with its own ring buffer,
dispatch tables, scheduler, and `ReceivesControl` trait — structurally
mirroring the MIDI pipeline but for non-MIDI control sources. Rejected
because MIDI CC already serves this purpose universally. DAWs, hardware
controllers, and web UIs all speak MIDI. Adding a parallel inbound mechanism
duplicates existing infrastructure, introduces a second ID namespace (event
IDs alongside MIDI CC numbers), and gains nothing that MIDI CC does not
already provide. The Voltage Modular precedent confirms that a MIDI-only
inbound approach is practical even for VST plugins.

### Per-module outbound ring buffers

Each emitting module gets its own ring buffer, sized to its traffic pattern.
Rejected in favour of a single shared ring buffer for simplicity: one
allocation, one drain loop, one consumer thread. The `ObservationSink`
abstraction allows splitting into per-type buffers later if needed.
