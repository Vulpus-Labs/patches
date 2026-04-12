---
id: "0350"
title: MasterSequencer host sync mode
priority: high
created: 2026-04-12
depends: "0348"
---

## Summary

Add a `sync` parameter to `MasterSequencer` with values `free` (default, current behaviour) and `host`. In `host` mode the sequencer reads the `HOST_TRANSPORT` backplane slot directly, deriving clock from host tempo and beat position, and starting/stopping with the host transport.

## Acceptance criteria

- [ ] New enum parameter `sync` with values `free` and `host`
- [ ] In `free` mode: behaviour is unchanged (own BPM, autostart, internal clock)
- [ ] In `host` mode: ignores `bpm` and `autostart` parameters
- [ ] In `host` mode: starts when `playing` lane transitions to 1.0, stops on 0.0
- [ ] In `host` mode: derives step clock from host tempo and beat position
- [ ] In `host` mode: correctly handles tempo changes mid-playback
- [ ] Doc comment updated with new parameter
- [ ] Tests for both sync modes
- [ ] `cargo test` and `cargo clippy` pass

## Notes

- MasterSequencer reads directly from the `GLOBAL_TRANSPORT` backplane slot — no explicit wiring from a HostTransport module is needed.
- `rows_per_beat` still applies in host mode — it determines how many sequencer steps per host beat.
- When host stops, sequencer position should freeze (not reset), matching DAW pause/resume behaviour.
