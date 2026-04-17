---
id: "0518"
title: Port patches-clap to patches-host
priority: medium
created: 2026-04-17
---

## Summary

Replace the inline composition in `patches-clap/src/plugin.rs` and
`patches-clap/src/factory.rs` with calls into `patches-host`. After
this ticket, the CLAP plugin code contains only CLAP-specific
concerns: transport/event extraction in the sample-accurate loop,
parameter handling, state save/restore, GUI wiring, and the cleanup
thread.

Part of epic E089 (see ADR 0040). Depends on 0516.

## Acceptance criteria

- [ ] `patches-clap`'s `compile_and_push_plan` uses `patches-host`'s
  patch-load helper instead of driving DSL pipeline stages inline.
- [ ] `Planner`, `Registry`, and processor construction come through
  `patches-host::HostBuilder`.
- [ ] CLAP-specific code stays in `patches-clap`: audio callback loop,
  CLAP event extraction, transport handling, param store, GUI state,
  cleanup thread.
- [ ] `patches-clap` does not depend on `patches-cpal` (CLAP hosts
  provide audio).
- [ ] Existing CLAP plugin behaviour unchanged: load, hot-reload on
  file edit (if wired), sample-accurate MIDI, transport sync, state
  save/restore all work as before.
- [ ] `cargo build`, `cargo test`, `cargo clippy` clean.

## Notes

The sample-accurate loop and CLAP transport/event extraction are the
parts that should *not* move into `patches-host`. Keep
`HostAudioCallback` deliberately abstract over the loop structure so
CLAP can implement it without constraining the sample-accurate
design.

GUI state and the cleanup thread are CLAP concerns and stay.

After this ticket, the epic's definition-of-done check can be run:
`cargo tree -p patches-lsp` must show no transitive dep on
`patches-engine`, `patches-planner`, or `cpal`.
