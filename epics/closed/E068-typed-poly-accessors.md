---
id: "E068"
title: Typed poly overlay accessors
created: 2026-04-12
tickets: ["0363", "0364"]
adr: "0033"
---

## Summary

Introduce zero-cost accessor structs for structured poly frames
(ADR 0033, Phase 1). `TransportFrame` provides named read/write
methods over the `GLOBAL_TRANSPORT` lane layout, replacing bare
`TRANSPORT_*` constants. `MidiFrame` defines the packing format
for up to 5 MIDI events in an `[f32; 16]` frame.

These accessors are the foundation for both the MIDI-over-poly
backplane (E069) and interpreter-level layout validation (E070).

## Tickets

| Ticket | Title                    |
| ------ | ------------------------ |
| 0363   | TransportFrame accessor  |
| 0364   | MidiFrame accessor       |

0363 and 0364 are independent of each other.
