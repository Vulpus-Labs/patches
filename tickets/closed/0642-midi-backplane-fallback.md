---
id: "0642"
title: Backplane fallback convention for midi input ports
priority: medium
created: 2026-04-23
---

## Summary

Establish convention: any module with a `midi` input port reads from the
backplane (default `GLOBAL_MIDI`) when the port is unconnected, and from
the upstream cable when it is. Source resolved at connection-update
time by setting the cable slot index on the module's `MidiInput`.
`process()` reads through one indirection always — no per-sample branch.

## Acceptance criteria

- [ ] `MidiInput` exposes a constructor / setter that takes a slot index
- [ ] Connection-update path (build + hot-reload) writes the right slot:
      upstream cable if connected, backplane slot otherwise
- [ ] Apply to existing MIDI consumers: `MidiToCv`, `PolyMidiToCv`,
      `MidiCc`, `MidiDrumset` (after rename — coordinate with 0643)
- [ ] Tests: a patch with no MIDI wiring still works (backplane);
      a patch wiring `MidiIn` → consumer overrides backplane
- [ ] Hot-reload test: connecting/disconnecting the port at reload
      switches source without restart

## Notes

ADR 0048. Depends on 0641.
