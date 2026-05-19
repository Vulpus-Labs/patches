---
id: "0930"
title: Reorg — sequencer/ group dir
priority: medium
created: 2026-05-18
epic: E151
---

## Summary

Create `patches-modules/src/sequencer/` and move the existing
`master_sequencer/`, `pattern_player/`, `tracker_core/`
subdirectories under it. The existing per-module directory
structure stays — this is one level of nesting added, not a
flattening.

## Acceptance criteria

- [ ] `patches-modules/src/sequencer/{master_sequencer/,
      pattern_player/, tracker_core/}` exist with the same internal
      contents as before; original locations removed.
- [ ] `patches-modules/src/sequencer/mod.rs` declares the
      submodules and re-exports the public types.
- [ ] Public re-exports preserve `patches_modules::MasterSequencer`,
      `::PatternPlayer`, `::TrackerCore` (and any other current
      public paths under these subdirs).
- [ ] Tests pass unchanged beyond import-path edits.
- [ ] `cargo clippy --all-targets -- -D warnings` green.
- [ ] `just commit -p patches-modules` green.

## Notes

Structural only. Mind the `patches-tracker-core` sibling crate
(referenced in `MEMORY.md`) if it exists — this ticket only touches
the module wrapper under `patches-modules/`, not the standalone
pure-logic crate.
