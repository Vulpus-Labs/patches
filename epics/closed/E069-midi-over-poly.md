---
id: "E069"
title: MIDI over poly backplane
created: 2026-04-12
tickets: ["0365", "0366", "0369", "0370", "0371", "0372"]
adr: "0033"
---

## Summary

Route MIDI events through the module graph as poly cable signals.
A new `GLOBAL_MIDI` backplane slot carries packed MIDI events
(using the `MidiFrame` format from E068). The engine writes
incoming MIDI into this slot each tick. All MIDI modules are
refactored to read MIDI from the backplane rather than via the
`ReceivesMidi` trait, making them pure graph modules and enabling
future MIDI transform/filter modules to be inserted upstream.
Once all modules are migrated, the `ReceivesMidi` trait and its
dispatch machinery are removed.

## Tickets

| Ticket | Title                                      |
| ------ | ------------------------------------------ |
| 0365   | MIDI backplane slot and engine integration |
| 0366   | PolyMidiIn reads MIDI from backplane       |
| 0369   | MonoMidiIn reads MIDI from backplane       |
| 0370   | MidiCc reads MIDI from backplane           |
| 0371   | MidiDrumset reads MIDI from backplane      |
| 0372   | Remove ReceivesMidi trait and dispatch     |

0365 must land first; 0366–0371 depend on it and can be done in any order.
0372 depends on all module migration tickets (0366–0371).
