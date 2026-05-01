---
id: "0777"
title: Internal preset library (save/load PersistedSettings)
priority: low
created: 2026-04-30
---

## Summary

A preset is `PersistedSettings` plus a patch identity reference
(file path or content hash). Save writes to a library directory
(XDG data dir, `presets/<patch-stem>/<preset-name>.json`). Load
applies via `Action::StateLoad` against the current patch.

Cross-patch loads work because keys are name-keyed; missing names
degrade gracefully (drop with status log entry).

## Acceptance criteria

- [ ] `Action::SavePreset { name }` and `Action::LoadPreset { path }`
      (or equivalent) wired through Controller.
- [ ] Library directory created on demand; preset list exposed via
      snapshot for UI rendering.
- [ ] Cross-patch load test: save preset against patch A, load
      against patch B, verify graceful degradation.
- [ ] Both shells can save and load presets.
- [ ] `cargo clippy` and `cargo test` pass.

## Notes

ADR 0063 §6. Depends on 0774, 0776. CLAP host preset browser
integration (`clap_plugin_preset_load`) is deferred.
