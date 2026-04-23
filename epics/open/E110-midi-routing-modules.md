---
id: "E110"
title: MIDI cable type and routing modules
created: 2026-04-23
tickets: ["0641", "0642", "0643", "0644", "0645", "0646", "0647", "0648"]
adrs: ["0048"]
---

## Goal

Split MIDI sourcing from interpretation. Add a typed `Midi` cable, a
pure source module, and a family of MIDI transformers (split, transpose,
arpeggiate, delay) that compose between source and consumers. Make
backplane fallback the standard convention for any module with a `midi`
input port, so existing patches keep working unchanged.

See ADR 0048 for the full design.

## Scope

1. **Cable type** — `CableKind::Midi` as a typed view over the existing
   `[f32; 16]` poly buffer; graph validation; `param_layout::port_kind_tag`.
2. **Backplane fallback convention** — any `midi` input port resolves to
   the backplane slot when unconnected, decided at connection-update
   time. No per-sample branch.
3. **Renames** — `MidiIn` → `MidiToCv`, `PolyMidiIn` → `PolyMidiToCv`,
   with registry aliases for one release.
4. **Source module** — new `MidiIn` reading the backplane, writing to a
   `midi` output port.
5. **Transformers** — `MidiSplit`, `MidiTranspose`, `MidiArp`,
   `MidiDelay`. All preserve non-note events (CC, pitch bend) sensibly
   and avoid stuck notes when dropping note-ons.

## Non-goals

- `MidiOut` egress (deferred; needs host-side sink in clap/engine).
- Channel filter, chord memory, scale quantiser — natural follow-ons,
  not in this epic.

## Tickets

- 0641 — Add `CableKind::Midi` core plumbing
- 0642 — Backplane fallback convention for `midi` input ports
- 0643 — Rename `MidiIn`/`PolyMidiIn` → `MidiToCv`/`PolyMidiToCv` with aliases
- 0644 — New `MidiIn` source module
- 0645 — `MidiSplit` keyboard splitter
- 0646 — `MidiTranspose` semitone shifter
- 0647 — `MidiArp` arpeggiator
- 0648 — `MidiDelay` pure MIDI delay
