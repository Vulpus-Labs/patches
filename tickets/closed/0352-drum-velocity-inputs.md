---
id: "0352"
title: Add velocity inputs to drum modules
priority: high
created: 2026-04-12
---

## Summary

Add a `velocity` mono input to all eight drum modules (Kick, Snare, ClosedHiHat, OpenHiHat, Tom, ClapDrum, Claves, Cymbal). When connected, velocity scales the output amplitude. When disconnected, output is at full amplitude (equivalent to velocity 1.0). This eliminates the need for external VCA wrappers for variable-velocity hits.

## Acceptance criteria

- [ ] Each drum module gains a `velocity` mono input
- [ ] When disconnected: output amplitude is unchanged (full volume)
- [ ] When connected: output is multiplied by the velocity value (0.0-1.0)
- [ ] Velocity is latched on trigger — the value at the moment of trigger is captured and held for the duration of the hit
- [ ] Doc comments updated for all eight modules
- [ ] Tests verify velocity scaling and disconnect-defaults-to-full behaviour
- [ ] `cargo test` and `cargo clippy` pass

## Notes

- Velocity should be latched at trigger time, not continuously modulated. This matches how real drum machines work — the velocity of a hit is set when it fires, not updated as the sound decays.
- The `drum_vca` template in `examples/drum_machine.patches` can be removed once this lands, simplifying the patch significantly.
- Accent behaviour (multi-parameter modulation beyond simple amplitude scaling) is deferred to a future ticket.
