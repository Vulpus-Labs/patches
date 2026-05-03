---
id: "E123"
title: CLAP file header + module paths tab (browse / reload / rescan)
created: 2026-04-26
tickets: ["0726", "0727", "0728", "0729"]
adrs: ["0044"]
---

## Goal

Add top-level patch file management (browse + reload) and a Modules
tab for managing module scan directories with rescan. All file /
directory pickers run on the main thread via `rfd`; rescan reuses the
existing hard-stop reload flow (ADR 0044 §3).

## Scope

1. File header strip: file path label + Browse + Reload buttons.
   Browse opens an `rfd` file picker, sets `GuiState::file_path`,
   triggers reload. Reload re-runs the existing pipeline.
2. Modules tab list: render `GuiState::module_paths`, with a per-row
   delete button driving `Intent::RemovePath { index }`.
3. Add Path: directory picker via `rfd` on the main thread, appends
   to `module_paths`, persisted via the existing state-save path.
4. Rescan button: triggers the hard-stop reload flow (ADR 0044 §3) so
   newly-added paths take effect.

## Out of scope

- Polish (E124)

## Acceptance

- Browse + Reload work end-to-end on macOS and Windows.
- Add / Remove path edits the persisted `module_paths` list.
- Rescan picks up new modules from added directories without
  restarting the host.
- `cargo clippy` and `cargo test` pass.
