---
id: "E065"
title: MIDI drum mapping and drum velocity inputs
created: 2026-04-12
tickets: ["0351", "0352"]
---

## Summary

Two independent improvements to the drum module ecosystem:

1. A `MidiDrumset` module that maps incoming MIDI notes to per-drum
   trigger and velocity output pairs following the General MIDI drum
   map, allowing a MIDI controller or DAW to drive drum modules
   directly.

2. Native `velocity` mono inputs on all eight drum modules (Kick,
   Snare, ClosedHiHat, OpenHiHat, Tom, ClapDrum, Claves, Cymbal).
   Velocity is latched at trigger time and scales output amplitude,
   eliminating the need for external VCA wrappers.

Together these enable a clean MIDI-driven drum patch:
`MidiDrumset` trigger/velocity outputs wired directly to drum module
inputs, with no intermediate VCA template.

## Tickets

| Ticket | Title                            |
|--------|----------------------------------|
| 0351   | MidiDrumset module               |
| 0352   | Add velocity inputs to drums     |

The tickets are independent and can be worked in parallel. 0352 is
useful on its own (simplifies tracker-driven patches); 0351 pairs
naturally with it for MIDI-driven use.
