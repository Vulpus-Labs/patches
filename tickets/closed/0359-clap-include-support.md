---
id: "0359"
title: CLAP plugin include support
priority: medium
created: 2026-04-12
epic: "E067"
depends: ["0357"]
---

## Summary

Update the CLAP plugin's `compile_and_push_plan()` to use the include loader when a file path is available, so that `include` directives in `.patches` files are resolved when loading or reloading in a DAW.

## Design

**`compile_and_push_plan()` change** (`patches-clap/src/plugin.rs`):

When `self.base_dir` is `Some` and a file path is available, use `load_with()` instead of `parse()`. The loader resolves includes relative to the master file's directory. Fall back to `parse()` when only an in-memory source string is available (e.g. state restored without original files on disk).

**State persistence** (`patches-clap/src/extensions.rs`):

No change to the persistence format. The saved `dsl_source` is the master file's source only. On restore:

- If the original file path exists, use `load_with()` to re-resolve includes from disk.
- If the file is missing, fall back to `parse()` on the saved source (master only, no includes — graceful degradation).

**Reload button:**

Reload is already manual (user clicks button). No dependency tracking needed — each reload re-resolves the full include tree.

## Acceptance criteria

- [ ] `compile_and_push_plan` uses `load_with` when file path is available
- [ ] Includes resolved relative to the master file's directory
- [ ] Falls back to `parse()` when no file path available (in-memory source only)
- [ ] State restore works with and without original files on disk
- [ ] Reload button re-resolves the full include tree
- [ ] Include errors reported in the GUI status line
- [ ] `cargo test` and `cargo clippy` pass

## Notes

- The CLAP plugin has no file watching, so there is no need to track dependency paths. The manual reload model means the user triggers a full re-resolution each time.
