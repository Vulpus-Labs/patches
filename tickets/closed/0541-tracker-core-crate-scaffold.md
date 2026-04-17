---
id: "0541"
title: Scaffold patches-tracker-core crate
priority: medium
created: 2026-04-17
epic: E092
---

## Summary

Create the `patches-tracker-core` crate as an empty sibling of
`patches-core` / `patches-dsp` / `patches-modules`. This ticket lands
the crate skeleton and workspace wiring. Scope and rationale are
recorded in [ADR 0042](../../adr/0042-tracker-core-crate.md). No
logic moves yet — extractions happen in 0542 and 0543.

## Acceptance criteria

- [x] New directory `patches-tracker-core/` with:
      - `Cargo.toml` (edition 2021, `publish = false` pending
        distribution decision, no features)
      - `src/lib.rs` containing a crate-level doc comment stating
        the crate's purpose and the "tracker is not DSP" boundary,
        plus `#![warn(clippy::pedantic)]`-consistent lints matching
        the rest of the workspace
- [x] `Cargo.toml` dependencies:
      - `patches-core` (path dep)
      - No audio-backend, no `patches-modules`, no `patches-dsp`,
        no `patches-registry`, no `cpal`, no `serde`
- [x] Workspace-root `Cargo.toml` adds `patches-tracker-core` to
      `[workspace] members` in alphabetical order.
- [x] `adr/0042-tracker-core-crate.md` in place documenting scope,
      boundary, and cross-reference to ADR 0040.
- [x] `cargo build -p patches-tracker-core` succeeds.
- [x] `cargo test --workspace` and `cargo clippy --workspace` clean.

## Notes

Pure scaffold. Subsequent tickets (0542, 0543) fill it.

ADR numbering continues from 0041 (E091's expander decomposition).

`ClockBusFrame` lives in this crate's `lib.rs` as a shared type
between `PatternPlayerCore` (0542) and `SequencerCore` (0543) —
decided during scaffold to avoid retrofitting in 0543.

Test-crate eligibility: the crate is usable from `patches-modules`
tests without `cfg(test)` gymnastics, since 0542 and 0543 call core
functions from module tests during the transition period.
