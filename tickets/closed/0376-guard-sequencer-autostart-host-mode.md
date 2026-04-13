---
id: "0376"
title: Guard MasterSequencer autostart in host mode
priority: medium
created: 2026-04-13
---

## Summary

In `patches-modules/src/master_sequencer.rs` lines 341–344,
`update_validated_parameters` immediately transitions to `Playing` and resets
position when `autostart` is set to true. In host mode this conflicts with the
transport — the sequencer starts playing before the host does, causing a brief
glitch.

## Acceptance criteria

- [ ] `autostart` transition is skipped when `self.use_host_transport` is true
- [ ] Document in the `autostart` parameter description that it is ignored in host sync mode
- [ ] Document that `swing` is ignored in host sync mode (currently undocumented)
- [ ] Test: in host mode, setting `autostart = true` does not start playback until host transport plays

## Notes

The `bpm` parameter is already documented as ignored in host mode. `autostart`
and `swing` should follow the same pattern.
