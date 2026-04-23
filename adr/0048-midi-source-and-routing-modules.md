# ADR 0048 — MIDI source and routing modules

**Date:** 2026-04-23
**Status:** proposed

---

## Context

Today `MidiIn` (mono-voice tracker) and `PolyMidiIn` (poly-voice tracker)
each do two jobs:

1. Pull MIDI events from a backplane slot (currently `GLOBAL_MIDI`).
2. Track note state and emit CV (`voct`, `gate`, `velocity`, ...).

This conflates *sourcing* MIDI with *interpreting* MIDI. It rules out any
transformer that sits between a source and a voice tracker — e.g. a
keyboard splitter that routes notes above a pitch to a lead tracker and
notes below to a chord tracker — because the voice trackers can only read
from the backplane and have nowhere to receive MIDI from another module.

`MidiCc` and `MidiDrumset` have the same shape: backplane reader fused
with interpretation.

Eventually we also want `MidiOut` (send MIDI to external gear or to a
plugin host's output port), but that needs a host-side egress path that
does not exist yet, so it is out of scope for this ADR.

## Decision

Split the responsibilities. Introduce a pure MIDI source module, a typed
MIDI cable, and rename the voice trackers to reflect that they are MIDI→CV
converters. Make the voice trackers and other interpreters fall back to
the backplane when their MIDI input port is unconnected, so existing
patches keep working without edits.

### Module renames

| Old name     | New name       | Role                       |
| ------------ | -------------- | -------------------------- |
| `MidiIn`     | `MidiToCv`     | Mono-voice MIDI→CV tracker |
| `PolyMidiIn` | `PolyMidiToCv` | Poly-voice MIDI→CV tracker |

`MidiCc` and `MidiDrumset` keep their names (their job is already
interpretation, not sourcing) but gain the same optional MIDI input port.

### New modules

- **`MidiIn`** — pure source. Reads from a backplane slot
  (default: `GLOBAL_MIDI`) and writes the events to a single `midi`
  output port. No voice state, no CV.
- **`MidiSplit`** — keyboard splitter. One `midi` input, two `midi`
  outputs (`low`, `high`), one `split` parameter (note number).
  Note-on/note-off events are routed by note number; non-note events
  (CC, pitch bend, ...) are forwarded to both outputs so downstream
  trackers see consistent controller state.
- **`MidiTranspose`** — semitone shift. One `midi` in, one `midi` out,
  one `semitones` int parameter (signed). Shifts note number on
  note-on/note-off; non-note events pass through. Notes that would shift
  out of `0..=127` are dropped (with a matching note-off if the
  corresponding note-on was dropped, to avoid stuck notes).
- **`MidiArp`** — arpeggiator. One `midi` in, one `midi` out, plus a
  `clock` trigger input (advances one step per pulse). Parameters:
  `pattern` (enum: up, down, up-down, random, as-played), `octaves`
  (1..=4), `gate_length` (fraction of clock period). Holds the set of
  currently-pressed notes; on each clock pulse emits note-off for the
  previous step and note-on for the next, walking the pattern. Empty
  hold set emits nothing. Pass-through for CC and pitch bend.
- **`MidiDelay`** — pure MIDI delay line. One `midi` in, one `midi`
  out, one `delay_samples` int parameter (or `delay_ms` float — pick
  one; samples is simpler and rt-safe). Buffers events with their
  arrival time and re-emits them after the delay. Non-note events
  delayed identically. Buffer is bounded; overflow drops oldest with
  a matching note-off if needed.

### Cable type

Add `CableKind::Midi` as a typed view over the existing poly buffer
layout. The `MidiFrame` packed encoding (ADR-era convention used by
`MidiInput` / `MidiOutput`) is unchanged; `Midi` is the same `[f32; 16]`
per sample with a distinct kind tag. This keeps the buffer pool, frame
size, and the existing debouncing logic in `MidiInput`/`MidiOutput`
intact while preventing a `Poly` audio output from being wired into a
MIDI input by accident.

The `port_kind_tag` table in `param_layout` gains a new variant; graph
validation in `graphs/graph` gains a `(Midi, Midi)` pair in the
compatibility match.

### Backplane fallback (standard for MIDI inputs)

Convention: **any module with a `midi` input port falls back to the
backplane when unconnected.** Applies to `MidiToCv`, `PolyMidiToCv`,
`MidiCc`, `MidiDrumset`, `MidiSplit`, and any future MIDI consumer.

The module's `MidiInput` holds a single cable slot index. On connection
update: if the port is connected, point it at the upstream cable's slot;
otherwise point it at the backplane slot (default `GLOBAL_MIDI`).
`process()` reads the same way regardless — no branch.

Effect: the first MIDI module in any chain pulls from the backplane
implicitly; later modules read from upstream. Patches that just want
"MIDI in, sound out" stay one-liners; patches that want routing wire
explicit `MidiIn` / `MidiSplit` chains. The pure `MidiIn` source module
is still useful when you want explicit provenance, multiple taps off
the same source, or a non-default backplane slot — but it isn't
mandatory boilerplate.

### Deferred: `MidiOut`

Listed here so the cable type and module split land with egress in
mind. `MidiOut` would consume a `midi` input and forward events to a
host-managed MIDI port. It needs:

- A host-side MIDI sink registered analogously to `GLOBAL_MIDI` ingress.
- CLAP/standalone egress wiring in `patches-clap` and `patches-engine`.

Neither exists today. Defer until a concrete consumer (external gear
control, sequencer-as-source patch) lands as a ticket.

## Consequences

- **Patch compatibility.** Old patches that name `MidiIn` / `PolyMidiIn`
  break at parse time. Either keep aliases in the module registry for one
  release, or do a one-shot rename pass over `patches/` and any test
  fixtures. Recommend aliases — cheap, and keeps user patches portable.
- **Splitter composability.** `MidiSplit` is the first of a family
  (channel filter, transpose, arpeggiator, chord memory, ...). Once the
  `Midi` cable type exists, these are all small modules with no new
  infrastructure.
- **Typed routing.** Wiring a `Poly` audio output to a `MidiToCv` input
  is a graph error rather than a silent garbage read.
- **No audio-thread cost.** Cable slot rewired on connection update;
  `process()` reads through the same indirection always.
- **Tests.** Module tests for the renamed converters keep their existing
  backplane-driven setup (the unconnected-port branch). New tests cover
  `MidiIn` → `MidiSplit` → `MidiToCv` chains end-to-end.

## Rejected alternatives

### Reuse `Poly` for MIDI cables

Works (the buffer is already `[f32; 16]`) but loses type safety: any
oscillator's poly output would be a legal source for a MIDI input.
Given how easy `CableKind::Midi` is to add, the type discipline is worth
the few lines.

### Keep the modules fused; add a "MIDI tee" instead

A tee that reads the backplane and forwards to a downstream port would
let a splitter sit downstream of a fused `PolyMidiIn`. But the splitter
still needs a *source-only* upstream module (the fused tracker is already
consuming), so the source/interpreter split is unavoidable. Doing it
once, cleanly, is simpler than introducing a tee and then later
unbundling anyway.
