---
id: "0594"
title: Migrate alloc_trap tests + deliberate-alloc negative test + CI job
priority: high
created: 2026-04-20
---

## Summary

Retire the local `TrappingAllocator` / `NoAllocGuard` copies in
`patches-integration-tests/tests/alloc_trap.rs` in favour of the
shared crate (tickets 0591–0593). Add a deliberate-alloc
negative test that confirms the trap actually aborts. Add a CI
job that runs `cargo test` with the
`audio-thread-allocator-trap` feature enabled so the mechanism
stays live in the default development loop.

## Acceptance criteria

- [ ] `patches-integration-tests/tests/alloc_trap.rs`:
      - Delete the local `TrappingAllocator`, thread-locals,
        and `NoAllocGuard`.
      - Import from `patches-alloc-trap`.
      - Install the global allocator conditionally on the
        `audio-thread-allocator-trap` feature; when the
        feature is off, the test suite runs against `System`
        and all `TRAP_HITS` assertions remain meaningful
        (counter stays at zero because the trap is inert).
      - Each sweep test continues to pass. Warm-up loops
        unchanged.
- [ ] New `#[test]` in a separate integration-test file that
      spawns a subprocess with the trap on, tags the thread,
      deliberately allocates a `Box::new(0u64)`, and asserts
      the subprocess aborts with a non-zero exit status. If
      implementing a subprocess harness is heavy, an
      equivalent alternative: a `#[test]` behind
      `#[cfg(feature = "audio-thread-allocator-trap")]` that
      uses `std::panic::catch_unwind` around a deliberately-
      allocating closure and asserts via `TRAP_HITS` — but
      only if the shim is changed to a counting-only soft
      mode for this one test (ADR 0045 suggests this is a
      valid future extension and this ticket can implement
      it as a test-only `set_mode(Count | Abort)` API).
- [ ] CI config gains a debug job that runs
      `cargo test --workspace --features
      patches-alloc-trap/audio-thread-allocator-trap` (exact
      feature path per whichever crate surfaces it). Job name
      explicit: `test-debug-alloc-trap`.
- [ ] Default CI jobs unchanged — the trap job is additive,
      not a replacement, so the workspace's normal cargo
      test keeps running without the feature.
- [ ] Trap-enabled CI run is green across the existing sweeps
      (simple, poly_synth, fm_synth, fdn_reverb_synth, pad,
      pentatonic_sah, drum_machine, tracker_three_voices).
- [ ] Documentation: a short note in the workspace README or
      `patches-alloc-trap/README.md` describing how to enable
      the trap locally (`cargo test -p … --features …`) and
      what it catches.

## Notes

The deliberate-alloc test is the correctness check for the
whole spike — without it, a silently-broken trap would leave
the whole suite "green" while catching nothing. A subprocess
harness with `std::process::Command` run from the test is the
cleanest shape: spawn `cargo run --example alloc_trap_check`
(a tiny example binary that marks itself, allocates, and
expects to abort) and assert the exit status.

Depends on 0591, 0592, 0593.
