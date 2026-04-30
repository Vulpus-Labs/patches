---
id: "0727"
title: Modules tab list with per-row remove
priority: medium
created: 2026-04-26
epic: "E123"
---

## Summary

Render `GuiState::module_paths` as a list on the Modules tab, with a
per-row delete button driving `Intent::RemovePath { index }`.

## Acceptance criteria

- [ ] Modules tab shows one row per path, in the persisted order.
- [ ] Each row has a delete affordance posting `RemovePath` with the
      correct index.
- [ ] `on_main_thread` drains the index, removes the entry, and
      persists the change via the existing state-save path.
- [ ] Empty state ("no scan paths configured") shown when the list
      is empty.
- [ ] `cargo clippy` and `cargo test` clean.
