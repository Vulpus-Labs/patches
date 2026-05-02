---
id: "0782"
title: Diagnose Audio→Trigger (and Trigger→Audio) cable mismatches
priority: medium
created: 2026-05-02
---

## Summary

Connecting an audio output to a trigger input (or vice versa) currently
type-checks. Both ports have `CableKind::Mono`; the audio-vs-trigger
distinction lives in `MonoLayout` and the cable validator does not
inspect it. A user wired `Lfo.square -> MidiArp.clock`; `TriggerInput`
treats every sample with `v > 0` as a pulse, so the high half of the
square fires the arp on every sample and no note ever sustains. The
patch validated cleanly and produced silence, with no clue as to why.

## Acceptance criteria

- [ ] `patches-check` emits a diagnostic when an audio-layout output
      feeds a trigger-layout input, and vice versa, on otherwise
      compatible mono / poly cables.
- [ ] LSP surfaces the same diagnostic at the connection's site.
- [ ] At least one regression test pinning a known-bad connection
      (e.g. `Lfo.square -> MidiArp.clock`) and one allowed pair
      (e.g. `Lfo.reset_out -> MidiArp.clock`).

## Notes

- Producers carry their `MonoLayout` in the `PortDescriptor`; the
  validator already has both endpoints. The check is local and cheap.
- Open question: should this be an error or a warning? Audio→trigger is
  almost always a bug, but a square LFO at sub-audio rates is a
  plausible-looking "clock" until you read the trigger semantics in
  ADR 0047. Erroring fits the rest of the connection-kind machinery.
- Symmetric concern for `PolyLayout` (Audio vs Trigger vs Midi) on poly
  cables — same fix shape, worth covering in the same pass.
- Original incident: `examples/fdn_reverb_synth.patches` (ticket
  conversation, 2026-05-02).
