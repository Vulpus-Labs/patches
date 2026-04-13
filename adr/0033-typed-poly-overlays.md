# ADR 0033 — Typed poly overlays

**Date:** 2026-04-12
**Status:** accepted (Phase 1 implemented in E068)

---

## Context

Poly cables carry `[f32; 16]` — sixteen lanes of untyped floating-point
data. Several current and planned uses treat this not as 16 independent
audio channels but as a structured frame encoding distinct named fields:

1. **Host transport (ADR 0031).** `GLOBAL_TRANSPORT` packs sample count,
   playing state, tempo, beat/bar position, triggers, and time signature
   into lanes 0–8. Readers index lanes by named constants
   (`TRANSPORT_PLAYING`, `TRANSPORT_TEMPO`, etc.) defined in
   `patches-core/src/cables.rs`.

2. **MIDI over poly (proposed).** A poly cable could carry up to 5
   packed MIDI events per sample (3 lanes each: status, data1, data2)
   plus an event count in lane 0. This would allow MIDI to enter via a
   backplane slot and flow through the module graph as a cable-routable
   signal, enabling MIDI transform/filter/merge modules that compose
   in chains before a terminal decoder like `PolyMidiIn`.

Both cases share the same problem: sender and receiver must agree on
which lane means what, with no compile-time or interpreter-level
enforcement. A mismatched connection (e.g. wiring a transport poly
output to a module expecting MIDI poly input) would produce silent
corruption, not an error.

## Decision

**Introduce typed poly overlays** — zero-cost accessor layers over
`[f32; 16]` that give lanes named, typed access points and carry a
layout identity that the interpreter can validate at wire-up time.

### 1. Named accessor structs (compile-time)

Each overlay is a unit struct with associated constants and
read/write methods:

```rust
pub struct TransportFrame;

impl TransportFrame {
    pub const SAMPLE_COUNT: usize = 0;
    pub const PLAYING: usize = 1;
    pub const TEMPO: usize = 2;
    pub const BEAT: usize = 3;
    pub const BAR: usize = 4;
    pub const BEAT_TRIGGER: usize = 5;
    pub const BAR_TRIGGER: usize = 6;
    pub const TSIG_NUM: usize = 7;
    pub const TSIG_DENOM: usize = 8;

    pub fn playing(frame: &[f32; 16]) -> bool {
        frame[Self::PLAYING] != 0.0
    }
    pub fn set_playing(frame: &mut [f32; 16], playing: bool) {
        frame[Self::PLAYING] = if playing { 1.0 } else { 0.0 };
    }
    // ... etc.
}

pub struct MidiFrame;

impl MidiFrame {
    pub const EVENT_COUNT: usize = 0;
    // Events packed as (status, data1, data2) triples starting at lane 1.
    pub const MAX_EVENTS: usize = 5;

    pub fn event_count(frame: &[f32; 16]) -> usize {
        frame[Self::EVENT_COUNT] as usize
    }
    pub fn read_event(frame: &[f32; 16], index: usize) -> MidiEvent {
        let base = 1 + index * 3;
        MidiEvent {
            bytes: [
                frame[base] as u8,
                frame[base + 1] as u8,
                frame[base + 2] as u8,
            ],
        }
    }
    pub fn write_event(frame: &mut [f32; 16], index: usize, event: MidiEvent) {
        let base = 1 + index * 3;
        frame[base] = event.bytes[0] as f32;
        frame[base + 1] = event.bytes[1] as f32;
        frame[base + 2] = event.bytes[2] as f32;
    }
    // ... etc.
}
```

This is the minimal change: it replaces scattered lane-index constants
with a single-point-of-definition accessor, produces no runtime cost,
and can be adopted incrementally. Existing code (`host_transport.rs`,
`master_sequencer.rs`, `processor.rs`) migrates from bare constants
to `TransportFrame::PLAYING` etc.

### 2. Port-level layout tags (interpreter-level)

Module descriptors gain an optional `poly_layout` tag on poly ports:

```rust
.poly_input("midi_in", PolyLayout::Midi)
.poly_output("transport", PolyLayout::Transport)
.poly_output("audio", PolyLayout::Audio)  // or None for untyped
```

The interpreter validates that connected poly ports share the same
layout. Layouts must match exactly — there is no wildcard. Since no
existing modules have declared typed-poly ports (they all use
hardcoded backplane slots), strict matching is safe and prevents
silent corruption from untyped connections to structured inputs.
The LSP can surface layout mismatches as diagnostics.

`PolyLayout` is a simple enum:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolyLayout {
    /// Untyped 16-channel audio/CV (default).
    Audio,
    /// Host transport frame (ADR 0031 lane layout).
    Transport,
    /// Packed MIDI events (count + up to 5 triples).
    Midi,
}
```

Layouts must match exactly. `Audio` connects only to `Audio`,
`Midi` only to `Midi`, etc. No existing modules declare typed-poly
ports (they use backplane slots), so strict matching introduces no
backward-compatibility risk.

### 3. Adoption strategy

The two layers are independent and can ship separately:

- **Phase 1: Accessor structs.** Introduce `TransportFrame` and
  `MidiFrame` in `patches-core`. Migrate existing transport code to
  use `TransportFrame` accessors. Implement MIDI-over-poly using
  `MidiFrame`. No interpreter changes needed.

- **Phase 2: Layout tags.** Add `PolyLayout` to `ModuleDescriptor`
  poly port definitions. Update the interpreter to validate layout
  compatibility on connection. Update the LSP to report mismatches.

Phase 1 is purely additive and can land immediately. Phase 2 is a
small interpreter change gated on there being enough typed-poly usage
to justify the validation.

## Alternatives considered

### Full typed port system

Replace `CableValue` with an enum carrying a type tag:
`Mono(f32)`, `Poly([f32; 16])`, `Transport(TransportState)`,
`Midi(MidiFrame)`, etc. This gives the strongest type safety but
requires changes throughout the engine (buffer pool, cable allocation,
plan building, every module's process method). The cost is
disproportionate to the benefit: there are currently two struct-over-
poly use cases, and `[f32; 16]` is the right physical representation
for all of them.

### Trait-based overlay

Define a `PolyOverlay` trait with `read`/`write` methods and use
`dyn PolyOverlay` or generics. Rejected because the overlay is a
compile-time mapping, not a runtime abstraction — associated constants
and free functions are simpler, faster, and easier to understand.

### Do nothing

Continue using bare lane-index constants. Workable for transport
(one struct, stable layout) but increasingly fragile as MIDI-over-poly
and potential future struct types (e.g. spectral frames, control
bundles) are added. The accessor layer is cheap enough to justify
even for the existing transport case alone.

## Consequences

- `TransportFrame` and `MidiFrame` become the canonical way to
  read/write structured poly data. Bare lane-index constants
  (`TRANSPORT_PLAYING`, etc.) are replaced by accessor methods.
- The physical cable type remains `[f32; 16]` everywhere. No engine
  changes are needed for Phase 1.
- MIDI can flow through the graph as a poly signal, entering via a
  backplane slot and reaching modules through normal cable routing.
  This enables MIDI transform modules (filter, transpose, merge,
  split) that compose in chains.
- `PolyMidiIn` can be refactored to read MIDI from a poly input
  port rather than via the `ReceivesMidi` trait, making it a pure
  graph module. The `ReceivesMidi` trait remains available for
  backward compatibility but is no longer the only path for MIDI
  into the graph.
- The `PolyLayout` enum is intentionally small and closed. Adding a
  new layout is an explicit decision (update the enum, add an
  accessor struct, document the lane layout). This is a feature:
  it keeps the number of structured poly types manageable.
