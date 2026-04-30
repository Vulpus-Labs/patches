---
id: "0728"
title: Add Path button — directory picker, append, persist
priority: medium
created: 2026-04-26
epic: "E123"
---

## Summary

Add Path button on the Modules tab. Opens an `rfd` directory picker
on the main thread, appends the selected directory to
`module_paths`, and persists. Does not rescan — that is 0729.

## Acceptance criteria

- [ ] Button posts `Intent::AddPath`.
- [ ] `on_main_thread` opens an `rfd::FileDialog::pick_folder()` and
      appends to `module_paths` if the user selects one.
- [ ] Cancelling the picker leaves state unchanged.
- [ ] Persisted state survives plugin reload.
- [ ] No automatic rescan — the user must press Rescan (0729).
- [ ] `cargo clippy` and `cargo test` clean.
