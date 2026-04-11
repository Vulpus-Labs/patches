---
id: "0318"
title: "patches-core: TrackerData, Pattern, Song, Step structs"
priority: high
created: 2026-04-11
---

## Summary

Define the runtime tracker data types in patches-core. These are the
structures that live inside `Arc<TrackerData>` and are read by modules on
the audio thread.

## Acceptance criteria

- [ ] `Step` struct: `cv1: f32`, `cv2: f32`, `trigger: bool`,
      `gate: bool`, `cv1_end: Option<f32>`, `cv2_end: Option<f32>`,
      `repeat: u8`
- [ ] `Pattern` struct: `channels: usize`, `steps: usize`,
      `data: Vec<Vec<Step>>` (indexed `[channel][step]`)
- [ ] `PatternBank` struct: `patterns: Vec<Pattern>` (indexed by bank
      index, alphabetical sort on name)
- [ ] `Song` struct: `channels: usize`,
      `order: Vec<Vec<usize>>` (`[row][channel]` → pattern bank index),
      `loop_point: usize`
- [ ] `SongBank` struct: `songs: HashMap<String, Song>`
- [ ] `TrackerData` struct: `patterns: PatternBank`, `songs: SongBank`
- [ ] All types are `Send + Sync` (required for `Arc` sharing)
- [ ] `cargo test -p patches-core` passes
- [ ] `cargo clippy -p patches-core` clean

## Notes

These types are optimised for audio-thread read access: flat arrays,
integer indexing, no strings in the hot path. The `PatternBank` is
indexed by integer; the name-to-index mapping is resolved at interpret
time and encoded into the `Song` order table.

This ticket has no DSL dependency — the types are defined by ADR 0029 and
can be implemented immediately.

Epic: E059
ADR: 0029
