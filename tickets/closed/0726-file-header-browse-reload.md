---
id: "0726"
title: File header strip — patch path, Browse, Reload
priority: high
created: 2026-04-26
epic: "E123"
---

## Summary

Add a top-level header strip above the tab bar: current patch path
label, Browse button, Reload button. Browse opens an `rfd` file
picker on the main thread; Reload re-runs the existing reload
pipeline.

## Acceptance criteria

- [ ] Header renders `GuiState::file_path` (or "no patch loaded").
- [ ] Browse button posts `Intent::Browse`; `on_main_thread` opens an
      `rfd::FileDialog` filtered to `.patches`, sets `file_path`, and
      kicks the reload flow.
- [ ] Reload button posts `Intent::Reload`; `on_main_thread` runs the
      existing reload pipeline.
- [ ] Cancelling the picker leaves state unchanged.
- [ ] Verified manually in Bitwig and Reaper on macOS.
- [ ] `cargo clippy` and `cargo test` clean.
