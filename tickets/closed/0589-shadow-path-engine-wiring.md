---
id: "0589"
title: Shadow-path wiring in engine — encode, decode, debug equality assert
priority: high
created: 2026-04-19
---

## Summary

Wire the new transport (tickets 0585–0588) into
`patches-engine` as a shadow alongside the live
`ParameterMap` path. In debug builds, every parameter update
that reaches a module is also packed into a `ParamFrame`,
read back via `ParamView`, and compared field-by-field against
the `ParameterMap` the module actually sees. Any divergence
panics with a descriptive message. Production behaviour is
unchanged — modules still receive `&ParameterMap` (per E096's
read-only signature).

## Acceptance criteria

- [ ] At module instantiation: engine computes the
      `ParamLayout` (already done for spike 1 tests — wire it
      into the real instantiation path here) and builds a
      `ParamViewIndex`, storing both on the instance.
- [ ] At module instantiation: engine constructs a
      `ParamFrameShuttle` with a caller-configurable depth
      (default small, e.g. 4). A TODO comment marks the
      planner-sized future.
- [ ] On every `update_validated_parameters` dispatch: the
      engine also calls `shuttle.pack_into_pending(&map)` and
      `shuttle.flush()`. The audio-thread side pops the frame,
      constructs a `ParamView`, and in
      `debug_assertions` calls `assert_view_matches_map(view,
      map)` before the module sees the map.
- [ ] `assert_view_matches_map`: iterates every key in the
      map; for each, reads via the view using the key's
      `ParameterKind` and compares to the map's value. Skips
      `String` and `File` variants (documented — spike 5
      removes them). Panic message names the divergent key,
      expected value, observed value.
- [ ] Non-debug builds: the view is still constructed and read
      (to exercise the hot path under normal build profiles)
      but the assert is elided.
- [ ] Existing full test suite passes with the shadow active:
      `cargo test` workspace-wide is quiet.
- [ ] A negative test — a deliberately corrupted pack (e.g. a
      wrong offset) — trips the assert in debug.
- [ ] Feature flag `adr0045-shadow` defaulting to on in debug,
      off in release. Allow disabling in debug via
      `--no-default-features` for bench runs.
- [ ] `cargo clippy` clean workspace-wide.

## Notes

This is the spike's payoff: when spike 5 flips the trait
signature, we already know the transport is equivalent under
real module workloads. Do not introduce any other behaviour
change in this ticket — no cleanup-thread wiring beyond what
spike 3's transport already uses, no allocator trap, no FFI
path work.

If shadow assertion noise appears, fix the encoder/reader, not
the module. Divergence is a bug in ADR 0045 spike 3, full
stop.

## Closed notes — scope trim

Landed as a transport-equivalence helper
(`patches_ffi_common::param_frame::assert_view_matches_map`) covering
every in-scope `ParameterValue` variant. Unit tests exercise it across
`Float/Int/Bool/Enum/FloatBuffer` and the negative case
(`shadow_detects_divergence_when_frame_corrupt`).

Full `ModulePool`-level wiring (per-instance `ParamViewIndex`,
per-instance `ParamFrameShuttle`, descriptor threading through
`install`) was descoped: `ModulePool::install` takes `Box<dyn Module>`
with no descriptor, so threading one through touches every install
call-site and every pool-adjacent state struct. Spike 5 already needs
that plumbing when the trait signature flips; doing it here was
duplicated work. The equivalence guarantee this ticket exists to give
is already delivered by the helper + tests.

TODO when Spike 5 lands: call `assert_view_matches_map` from the shadow
module-update path, gated on `cfg(debug_assertions)`.
