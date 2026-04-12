---
id: "0358"
title: Player multi-file hot-reload
priority: medium
created: 2026-04-12
epic: "E067"
depends: ["0357"]
---

## Summary

Update `patches-player` to use the include loader and watch all files in the dependency set for changes, not just the master file.

## Design

**`load_patch` change** (`patches-player/src/main.rs`):

Replace direct `parse()` + `expand()` with `load_with()` + `expand()`. The function returns both the `BuildResult` and the `Vec<PathBuf>` of dependencies from `LoadResult`.

**Hot-reload loop:**

Currently polls mtime of a single path. Change to:

- Maintain `HashMap<PathBuf, SystemTime>` of watched paths and their last-known mtimes.
- On each poll cycle, check all watched paths. If any has changed, reload the entire include tree via `load_with`.
- On successful reload, refresh the watched-paths map from the new `LoadResult::dependencies` (includes may have been added or removed).
- On failed reload, keep the current watched set (so the user can fix the error and the change will be detected).

## Acceptance criteria

- [ ] `load_patch` uses `load_with` and returns dependency paths
- [ ] Hot-reload triggers when any included file is modified
- [ ] Dependency set refreshes on each successful reload (new includes detected)
- [ ] Parse errors in included files are reported with file path context
- [ ] Removing an include from the master file stops watching the removed file
- [ ] `cargo test` and `cargo clippy` pass

## Notes

- The reload granularity remains coarse: any file change reloads the entire tree. Fine-grained incremental reload is not needed at this stage.
- The 500ms poll interval is unchanged.
