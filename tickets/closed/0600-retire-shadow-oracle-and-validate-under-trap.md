---
id: "0600"
title: Retire shadow oracle; validate full suite under allocator trap
priority: high
created: 2026-04-20
depends_on: ["0596", "0597", "0598", "0599"]
---

## Summary

Once `ParamView` is the production path, the Spike 3 shadow
oracle is redundant. Remove it and lock in the migration by
running the whole workspace test suite under the Spike 4
allocator trap.

## Scope

- Delete `assert_view_matches_map` and its call sites.
- Delete [patches-ffi-common/src/param_frame/shadow.rs](../../patches-ffi-common/src/param_frame/shadow.rs).
  Keep `ParamFrame`, `pack_into`, `ParamView`, `ParamViewIndex`
  — those are now load-bearing.
- Remove any engine / module test hooks that fed the oracle.
- Run `cargo test --workspace --features
  patches-alloc-trap/audio-thread-allocator-trap` and fix any
  audio-thread allocation uncovered by the trap. None are
  expected (Spike 3 already exercised the pack path
  allocation-free), but the sweep patches in
  [patches-integration-tests/tests/alloc_trap.rs](../../patches-integration-tests/tests/alloc_trap.rs)
  now run under the full new data plane, not the old
  `ParameterMap` path.

## Acceptance criteria

- [ ] `shadow.rs` and `assert_view_matches_map` gone; workspace
      compiles.
- [ ] `cargo test --workspace` green (feature off).
- [ ] `cargo test --workspace --features
      patches-alloc-trap/audio-thread-allocator-trap` green.
- [ ] `patches-player` runs ≥10 s real playback with the trap
      armed, end-to-end, on the reference demo patch, without
      aborting.
- [ ] `cargo clippy --workspace` clean.
- [ ] Grep confirms no `&ParameterMap` on any audio-thread entry
      path.

## Non-goals

- FFI ABI changes (spike 7).
- Port bindings (`PortFrame` / `PortView`).
- `ArcTable` growth (spike 6).
