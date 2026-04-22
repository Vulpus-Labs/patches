---
id: "E103"
title: Sub-sample trigger cables and oscillator hard sync
created: 2026-04-22
tickets: ["0632", "0633", "0634", "0635", "0636", "0637", "0638"]
adrs: ["0047"]
---

## Goal

Add sub-sample-accurate hard sync to all phase-accumulator oscillators
by introducing a typed `Trigger` / `PolyTrigger` cable kind that carries
the fractional position of an event within a sample. Sync edges are
then PolyBLEP-correct at their true sub-sample position, eliminating
the aliasing baked in by sample-boundary-rounded threshold detection.

See ADR 0047 for the full design. In short: `reset_out` emits
`0` silent / `frac ∈ (0, 1]` on wrap; `sync` in consumes the same
encoding and resets the slave phase with BLEP applied at offset
`1 - frac`, scaled by each waveform's pre→post jump.

## Scope

1. **Core plumbing** — `CableKind::Trigger` / `PolyTrigger`, buffer
   reuse of Mono/Poly layouts, graph validation, builder, harness,
   `param_layout::port_kind_tag`.
2. **Consumer wrappers** — `SubTriggerInput` / `PolySubTriggerInput`
   returning `Option<f32>`; no prev-state, no threshold.
3. **Converters** — `TriggerToSync` and `SyncToTrigger` modules for
   explicit crossing to/from the ADR 0030 0.5-threshold world.
4. **Oscillator ports** — `reset_out` + `sync` on `VDco`, `VPolyDco`,
   `Osc`, `PolyOsc`, `Lfo`. Sub-sample BLEP reset logic in each.
5. **Integration test** — a patch chaining master → slave oscillator
   showing reduced residual aliasing vs a baseline patched through a
   `TriggerToSync` converter from a threshold-detected 0/1 pulse.

## Non-goals

- Phase-typed `(phase, dt)` compound cables (rejected in ADR 0047).
- Sub-sample gates (release-edge precision; deferred — see ADR 0047).
- Retriggerable envelopes and sub-sample clock dividers consuming
  `Trigger` cables. These are natural follow-ons, not part of this
  epic.

## Tickets

- 0632 — Add `CableKind::Trigger` / `PolyTrigger` core plumbing
- 0633 — `SubTriggerInput` / `PolySubTriggerInput` consumer wrappers
- 0634 — `TriggerToSync` / `SyncToTrigger` converter modules
- 0635 — `reset_out` + `sync` on `VDco` and `VPolyDco`
- 0636 — `reset_out` + `sync` on `Osc`, `PolyOsc`, `Lfo`
- 0637 — Hard-sync integration test and aliasing comparison
- 0638 — VDco/VPolyDco sync softness (RC-discharge model)
