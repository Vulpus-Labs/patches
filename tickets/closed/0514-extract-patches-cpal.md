---
id: "0514"
title: Extract patches-cpal crate from patches-engine
priority: medium
created: 2026-04-17
---

## Summary

Move the cpal stream creation, audio callback, and cpal input capture
out of `patches-engine` into a new `patches-cpal` crate. After this
ticket, `patches-engine` has no `cpal` dependency and is usable by
non-cpal embeddings (CLAP, offline render, tests) without pulling
desktop audio I/O.

Part of epic E089 (see ADR 0040).

## Acceptance criteria

- [ ] New `patches-cpal/` crate exists with `publish = false`.
- [ ] `cpal = "0.15"` dependency moves from `patches-engine/Cargo.toml`
  to `patches-cpal/Cargo.toml`.
- [ ] Moved: `patches-engine/src/callback.rs`,
  `patches-engine/src/input_capture.rs`.
- [ ] `patches-engine/src/engine.rs` splits: backend-agnostic setup
  (sample rate handling, env construction, processor spawn) stays in
  `patches-engine`; cpal stream creation and device negotiation move
  to `patches-cpal`.
- [ ] `patches-cpal` depends on `patches-engine`, not the reverse.
- [ ] `patches-player` depends on `patches-cpal`; `patches-clap` does
  not.
- [ ] `grep 'cpal' patches-engine/Cargo.toml patches-engine/src/**/*.rs`
  is empty.
- [ ] Decide and record: does MIDI (`patches-engine/src/midi/`) stay in
  engine or move? If cross-embedding, stays. If cpal-coupled, moves.
  Document the decision in the PR description.
- [ ] `wav_recorder.rs` moves to `patches-io` (which already exists).
- [ ] `cargo build`, `cargo test`, `cargo clippy` clean.

## Notes

No behaviour change. Player should produce identical audio output
before and after.

`patches-engine/src/engine.rs` is the most delicate file: cpal stream
creation lives alongside backend-agnostic processor setup. Split
carefully along that seam rather than moving the whole file.

**MIDI decision (2026-04-18):** `patches-engine/src/midi/` has no cpal
coupling, so it stays in `patches-engine`. `AudioClock`, `EventQueue`,
`MidiConnector`, `EventScheduler` remain available to non-cpal
embeddings (CLAP uses them directly).

**PatchEngine followed the split:** `PatchEngine` / `PatchEngineError`
owned a `SoundEngine`, so they moved to `patches-cpal::patch_engine`.
`Planner` stays in `patches-engine`. `CleanupAction` /
`DEFAULT_MODULE_POOL_CAPACITY` extracted to
`patches-engine/src/cleanup.rs`.
