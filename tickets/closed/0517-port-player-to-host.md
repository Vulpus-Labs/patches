---
id: "0517"
title: Port patches-player to patches-host and patches-cpal
priority: medium
created: 2026-04-17
---

## Summary

Replace `patches-player`'s inline composition with calls into
`patches-host` and `patches-cpal`. After this ticket, `main.rs`
contains only the integration layer: CLI handling, file-watch hot
reload, MIDI device connector, and the audio output glue.

Part of epic E089 (see ADR 0040). Depends on 0514 and 0516.

## Acceptance criteria

- [ ] `patches-player/src/main.rs` no longer constructs `Planner`,
  `Registry`, or `PatchEngine` directly; those come through
  `patches-host`.
- [ ] Audio callback and cpal stream come through `patches-cpal`.
- [ ] File-watch hot-reload calls into `patches-host`'s patch-load
  helper instead of driving the pipeline stage-by-stage inline.
- [ ] `patches-player`'s dependencies in `Cargo.toml` collapse to:
  `patches-host`, `patches-cpal`, `patches-diagnostics`, `ariadne`,
  and any CLI-specific deps. Everything else goes via transitive
  dependencies through `patches-host` and `patches-cpal`.
- [ ] `patch_player <file>` loads and plays identically to before
  (sample-for-sample or close to it, accounting for non-determinism
  in startup timing).
- [ ] Hot-reload behaviour unchanged.
- [ ] `cargo build`, `cargo test`, `cargo clippy` clean.

## Notes

No behaviour change. This is where the `patches-host` trait shape
gets its first real workout; if the traits are awkward to implement
from the player side, refine them in `patches-host` and update this
ticket rather than adding workarounds in the player.

Diagnostic-render (`patches-player/src/diagnostic_render.rs`) stays
in the player until/unless a shared rendering concern emerges in
`patches-host` or `patches-diagnostics`.
