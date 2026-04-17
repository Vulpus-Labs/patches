---
id: "0515"
title: Slim patches-engine to backend-agnostic kernel
priority: medium
created: 2026-04-17
---

## Summary

With planner extracted (0513) and cpal extracted (0514),
`patches-engine` should contain only the kernel, executor, module
pool, execution state, processor, and backend-agnostic engine shell.
Audit what remains and remove anything that has crept in.

Part of epic E089 (see ADR 0040). Depends on 0513 and 0514.

## Acceptance criteria

- [ ] `patches-engine/src/` contains only: `lib.rs`, `kernel.rs`,
  `execution_state.rs`, `pool.rs`, `processor.rs`, `decimator.rs`,
  `oversampling.rs`, a slimmed `engine.rs`, and any remaining
  backend-agnostic helpers.
- [ ] No `cpal`, `notify`, file I/O, or DSL-parsing dependencies in
  `patches-engine/Cargo.toml`.
- [ ] `patches-engine` depends on: `patches-core`, `patches-dsp`,
  `patches-planner`, `patches-registry`. Any additional deps need an
  explicit justification in the PR.
- [ ] `cargo tree -p patches-engine` shows the minimal kernel
  dependency set.
- [ ] All engine unit and integration tests still pass.
- [ ] `cargo build`, `cargo test`, `cargo clippy` clean.

## Notes

This ticket is primarily a verification pass after 0513 and 0514; most
of the actual moves happen in those two. Use this ticket to sweep up
anything that was missed, tighten `Cargo.toml`, and confirm the
dependency surface matches the intent in ADR 0040.

If MIDI routing (`patches-engine/src/midi/`) is found to be
cpal-coupled during 0514, it will have been moved; if it was kept, it
stays here. Either way, document the final state in the PR.
