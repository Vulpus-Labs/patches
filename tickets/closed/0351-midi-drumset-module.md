---
id: "0351"
title: MidiDrumset module
priority: high
created: 2026-04-12
---

## Summary

Add a `MidiDrumset` module that receives MIDI note events and maps them to per-drum trigger and velocity output pairs following the General MIDI drum map. This allows a MIDI controller or DAW to drive the drum modules directly.

## GM drum mapping

| Note | Output name | Typical instrument |
|------|-------------|--------------------|
| 36   | kick        | Bass Drum 1        |
| 38   | snare       | Acoustic Snare     |
| 39   | clap        | Hand Clap          |
| 42   | closed_hh   | Closed Hi-Hat      |
| 44   | pedal_hh    | Pedal Hi-Hat       |
| 46   | open_hh     | Open Hi-Hat        |
| 41   | tom_low     | Low Floor Tom      |
| 45   | tom_mid     | Low Tom            |
| 48   | tom_high    | Hi-Mid Tom         |
| 49   | crash       | Crash Cymbal 1     |
| 51   | ride        | Ride Cymbal 1      |
| 75   | claves      | Claves             |
| 56   | cowbell     | Cowbell            |
| 37   | rimshot     | Side Stick         |

## Acceptance criteria

- [ ] Module registered as `MidiDrumset`
- [ ] Implements `ReceivesMidi` trait
- [ ] For each mapped note: a `trigger_<name>` mono output (1.0 pulse on note-on) and a `velocity_<name>` mono output (0.0-1.0)
- [ ] Configurable MIDI channel parameter (default: any channel, i.e. responds to all)
- [ ] Unmapped notes are ignored
- [ ] Note-on with velocity 0 treated as note-off (no trigger)
- [ ] Doc comment follows module documentation standard
- [ ] Tests verify trigger pulses and velocity values for mapped notes
- [ ] `cargo test` and `cargo clippy` pass

## Notes

- The output list is fixed at compile time (not configurable per-patch). This keeps port descriptors zero-cost.
- Consider whether to also accept note 35 (Acoustic Bass Drum) as an alias for kick.
- Pedal hi-hat (44) could be wired to closed_hh choke or used independently.
