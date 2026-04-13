---
id: "0350"
title: MasterSequencer host sync mode
priority: high
created: 2026-04-12
depends: "0348"
---

## Summary

Add a `sync` parameter to `MasterSequencer` with values `auto`
(default), `free`, and `host`. In `auto` mode the sequencer checks
`AudioEnvironment::hosted` once in `prepare` to decide its clock
source — host transport if hosted, internal BPM otherwise. `free`
forces internal clock; `host` forces host transport. In
host-derived modes the sequencer reads the `GLOBAL_TRANSPORT`
backplane slot directly.

## Acceptance criteria

- [ ] New enum parameter `sync` with values `auto`, `free`,
      and `host`
- [ ] In `auto` mode: reads `AudioEnvironment::hosted` in
      `prepare` to select clock source
- [ ] In `free` mode: behaviour is unchanged (own BPM,
      autostart, internal clock)
- [ ] In `host` mode: ignores `bpm` and `autostart` parameters
- [ ] In `host` mode: starts when `playing` lane transitions
      to 1.0, stops on 0.0
- [ ] In `host` mode: derives step clock from host tempo and
      beat position
- [ ] In `host` mode: correctly handles tempo changes
      mid-playback
- [ ] Doc comment updated with new parameter
- [ ] Tests for all three sync modes
- [ ] `cargo test` and `cargo clippy` pass

## Notes

- `auto` is the default so that patches work correctly in both
  standalone and hosted contexts without modification.
- The clock source decision is made once in `prepare` based on
  `AudioEnvironment::hosted` — no per-sample or periodic
  detection needed.
- MasterSequencer reads directly from the `GLOBAL_TRANSPORT`
  backplane slot — no explicit wiring from a HostTransport
  module is needed.
- `rows_per_beat` still applies in host mode — it determines
  how many sequencer steps per host beat.
- When host stops, sequencer position should freeze (not
  reset), matching DAW pause/resume behaviour.
