---
id: "0776"
title: Wire sidecar lifecycle (load on LoadPath, debounced save on dirty)
priority: medium
created: 2026-04-30
---

## Summary

Hook the sidecar `Env` methods into the controller and Ratatui shell
loop:

- After successful `LoadPath` compile, controller queries
  `Env::sidecar_path` and, if a sidecar exists, applies its
  `PersistedSettings` via the same path as `Action::StateLoad`.
  Missing sidecar → defaults.
- Ratatui shell loop watches `StateDelta::persistable_changed` and
  schedules a debounced `save_sidecar` (suggested 500ms window).
- CLAP shell on `persistable_changed` calls `mark_state_dirty` on
  the host (existing CLAP API); no sidecar write.

Stale entries (renamed/removed knobs, taps) are dropped on the next
save after manifest reconciliation. Pre-0057 there is no manifest;
the drop logic activates with 0057.

## Acceptance criteria

- [ ] Loading a patch with an adjacent sidecar restores its settings.
- [ ] Editing a setting in Ratatui causes one debounced save (not
      one save per keystroke).
- [ ] CLAP shell calls `mark_state_dirty` on dirty deltas.
- [ ] Sidecar load failure is non-fatal; status-logged.
- [ ] Integration test: create sidecar, load patch, verify state;
      mutate, wait for debounce, verify written file.
- [ ] `cargo clippy` and `cargo test` pass.

## Notes

ADR 0063 §5. Depends on 0772, 0774, 0775.
