---
id: "0541"
title: Scaffold patches-tracker-core crate and write ADR 0042
priority: medium
created: 2026-04-17
epic: E092
---

## Summary

Create the `patches-tracker-core` crate as an empty sibling of
`patches-core` / `patches-dsp` / `patches-modules`. This ticket lands
the crate skeleton, workspace wiring, and an ADR recording the
rationale and scope boundary. No logic moves yet — extractions
happen in 0542 and 0543.

## Acceptance criteria

- [ ] New directory `patches-tracker-core/` with:
      - `Cargo.toml` (edition 2021, `publish = false` pending
        distribution decision, no features)
      - `src/lib.rs` containing a crate-level doc comment stating
        the crate's purpose and the "tracker is not DSP" boundary,
        plus `#![warn(clippy::pedantic)]`-consistent lints matching
        the rest of the workspace
      - No other modules
- [ ] `Cargo.toml` dependencies:
      - `patches-core` (path dep)
      - No audio-backend, no `patches-modules`, no `patches-dsp`,
        no `patches-registry`, no `cpal`, no `serde`
- [ ] Workspace-root `Cargo.toml` adds `patches-tracker-core` to
      `[workspace] members` in alphabetical order.
- [ ] New file `adr/0042-tracker-core-crate.md` documenting:
      - Why a new crate vs. adding to `patches-core` or `patches-dsp`
      - What belongs here (pattern/song advance logic, transport
        state machines, step-timing calculations, clock-bus
        encoding/decoding, pattern-player step logic)
      - What does not belong here (anything depending on `CablePool`,
        `Module` trait, `GLOBAL_TRANSPORT`, or concrete audio I/O;
        anything doing signal processing)
      - Explicit ADR cross-reference to 0040 (kernel carve) as
        precedent
- [ ] `cargo build -p patches-tracker-core` succeeds and produces
      an empty `rlib`.
- [ ] `cargo test --workspace` and `cargo clippy --workspace` clean.

## Notes

This is a pure scaffold. No public items, no re-exports, nothing
for consumers to depend on yet. The subsequent tickets (0542, 0543)
fill it.

ADR numbering continues from 0041 (E091's expander decomposition).

Test-crate eligibility: the crate must be usable from
`patches-modules` tests without `cfg(test)` gymnastics, since 0542
and 0543 will need to call core functions from module tests during
the transition period.
