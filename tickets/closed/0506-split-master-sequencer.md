---
id: "0506"
title: Split patches-modules master_sequencer/mod.rs
priority: medium
created: 2026-04-16
---

## Summary

`patches-modules/src/master_sequencer/mod.rs` is 629 lines after
the E085 test extraction. The remaining impl covers parameter
handling, song/pattern playback state, and tracker-data lookup.

## Acceptance criteria

- [ ] Add sibling submodules inside `master_sequencer/`:
      `params.rs` (param registration / parsing / song-slot
      plumbing), `playback.rs` (step advancement + row/tick state
      machine), and, if the structure justifies it, a `lookup.rs`
      covering tracker-data resolution.
- [ ] `Module` impl stays in `mod.rs`, delegating to submodule
      helpers.
- [ ] `mod.rs` under ~400 lines.
- [ ] `cargo build -p patches-modules`, `cargo test -p patches-modules`,
      `cargo clippy` clean.

## Notes

E086. Confirm exact submodule boundaries on opening the file; the
shape above is the intended target.
