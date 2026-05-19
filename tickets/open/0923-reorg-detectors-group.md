---
id: "0923"
title: Reorg — detectors/ group dir
priority: medium
created: 2026-05-18
epic: E151
---

## Summary

Create `patches-modules/src/detectors/` and move
`trigger_sync_conv` into it. The new modules from tickets 0917 /
0918 (`AudioToTrigger` family, `AudioToGate` family) live here; if
they have landed in the flat layout, this ticket moves them.

Subfiles per variant
(`detectors/audio_to_trigger.rs`,
`detectors/stereo_audio_to_trigger.rs`,
`detectors/poly_audio_to_trigger.rs`, ditto for gate;
`detectors/trigger_sync_conv.rs`). A `detectors/common/` submodule
holds the shared edge-detector kernel from 0917.

## Acceptance criteria

- [ ] `patches-modules/src/detectors/` exists; flat
      `trigger_sync_conv.rs` deleted.
- [ ] Public re-exports preserve `patches_modules::TriggerSyncConv`
      and the new types.
- [ ] Tests pass unchanged beyond import-path edits.
- [ ] `cargo clippy --all-targets -- -D warnings` green.
- [ ] `just commit -p patches-modules` green.

## Notes

Structural only.
