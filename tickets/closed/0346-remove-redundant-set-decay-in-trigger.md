---
id: "0346"
title: Remove redundant set_decay calls from drum trigger blocks
priority: medium
created: 2026-04-12
---

## Summary

Every drum module calls `set_decay` in its trigger block with values that
are already set during `set_parameters`. This recomputes an `exp()` on
every trigger for no reason — the decay time hasn't changed.

Some modules also call `set_decay` with hardcoded constants in trigger
blocks (kick `click_env.set_decay(0.003)`, snare `snap_env.set_decay(0.005)`,
tom `noise_env.set_decay(0.01)`). These never change and only need to be set
once in `prepare`.

## Changes

- Remove `set_decay` calls from trigger blocks where the value matches
  what was already set in `set_parameters`.
- For constant-decay envelopes, set the value once in `prepare` and never
  again.
- `DecayEnvelope` itself needs no API change — the separation of
  `set_decay` from triggering is already correct in the type. The problem
  is how callers use it.

## Affected modules

- `kick.rs`: `amp_env.set_decay` (redundant), `click_env.set_decay(0.003)` (constant)
- `snare.rs`: `body_env.set_decay`, `noise_env.set_decay` (redundant), `snap_env.set_decay(0.005)` (constant)
- `clap_drum.rs`: `tail_env.set_decay` (redundant)
- `claves.rs`: `amp_env.set_decay` (redundant)
- `tom.rs`: `amp_env.set_decay` (redundant), `noise_env.set_decay(0.01)` (constant)
- `cymbal.rs`: `amp_env.set_decay` (redundant)
- `hihat.rs`: both OpenHiHat and ClosedHiHat `amp_env.set_decay` (redundant)

## Acceptance criteria

- [ ] No `set_decay` calls remain in trigger blocks
- [ ] Constant-decay envelopes configured once in `prepare`
- [ ] `cargo test -p patches-modules` passes
- [ ] `cargo clippy -p patches-modules` clean

## Notes

Epic E063. Can be done independently of E062.
