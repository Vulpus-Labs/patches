# E060 — Tracker modules

## Goal

Implement the `MasterSequencer` and `PatternPlayer` modules described in
ADR 0029, and verify the full round-trip (DSL → interpreter → plan →
audio) with integration tests.

After this epic:

- `MasterSequencer` drives song playback with transport controls, swing,
  and a poly clock bus per song channel.
- `PatternPlayer` reads a poly clock bus, steps through pattern data,
  and outputs cv1/cv2/trigger/gate per channel. Supports slides and
  repeats.
- Integration tests verify pattern data flows correctly from a `.patches`
  file through to module outputs.

## Background

ADR 0029 describes the full design. Both modules implement
`ReceivesTrackerData` (E059) and receive `Arc<TrackerData>` at plan
activation. The MasterSequencer reads song data; the PatternPlayer reads
pattern data. The clock bus is a poly signal carrying pattern reset,
pattern bank index, tick trigger, and tick duration.

## Tickets

| ID   | Title                                              | Dependencies       |
| ---- | -------------------------------------------------- | ------------------ |
| 0324 | MasterSequencer module                              | 0319, 0323         |
| 0325 | PatternPlayer module                                | 0319, 0323         |
| 0326 | Integration tests: tracker sequencer round-trip     | 0324, 0325, 0321   |

Epic: E060
ADR: 0029
