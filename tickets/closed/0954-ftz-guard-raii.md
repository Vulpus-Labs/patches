---
id: "0954"
title: FtzGuard — RAII save/restore for denormal flush mode at the block boundary
priority: medium
created: 2026-05-24
---

## Summary

E134 set hardware FTZ/DAZ at the top of each audio callback (0802) and added
per-site `flush_denormal()` calls in IIR kernels as defense in depth (0805).
At runtime on the single audio thread the kernel flushes are redundant: FTZ is
already set, so the hardware flushes subnormal results regardless. The two
layers exist because the invariant "subnormal ops are cheap on this thread" has
no single owner — it is reasserted both at the callback and inside each kernel,
and *neither* covers the offline paths (integration tests, profiling benches)
that run the same kernels with no FTZ set.

Replace the bare `enable_flush_to_zero()` call with an RAII guard that owns the
invariant at one place: the block handler. `FtzGuard::enable()` saves the
current MXCSR (x86) / FPCR (aarch64), ORs in FTZ|DAZ, and on `Drop` restores the
saved value — true try/finally semantics. Holding the guard for the duration of
a block makes FTZ a guaranteed property of "running the DSP graph for this
block" on whatever thread constructs it, which is the precondition for later
deleting the scattered kernel flushes (follow-up, not this ticket).

This is not a perf change (one register write/buffer, unchanged) and not an
audio change (subnormals are inaudible either way). It is a structural change:
single owner of the FTZ invariant, plus correct restore on panic unwind.

## Design

```rust
/// Enables hardware flush-to-zero / denormals-as-zero for the lifetime of the
/// guard, restoring the previous mode on drop. Construct once at the top of a
/// block handler; bind to a named local so it lives for the whole block.
pub struct FtzGuard {
    saved: u32, // MXCSR on x86; low bits of FPCR on aarch64; unused otherwise
}

impl FtzGuard {
    #[inline]
    pub fn enable() -> Self { /* read + save, then set FTZ|DAZ */ }
}

impl Drop for FtzGuard {
    #[inline]
    fn drop(&mut self) { /* write saved value back */ }
}
```

- Lives in `patches-dsp` alongside the current `enable_flush_to_zero()` (which
  it supersedes for callers; keep or remove the free fn — see criteria).
- Save-and-restore, not set-and-leave. The audio thread may be the host's
  thread (CLAP); leaving FTZ flipped is impolite even if E134 notes hosts often
  reset MXCSR themselves. RAII without restore is pointless.
- Block-level only. Construct outside the per-sample `tick()` loop — one
  register write per buffer, same cost as today. Never per sample.
- aarch64: save/restore the whole FPCR (`mrs`/`msr`), set bit 24 (FZ). No
  separate DAZ on aarch64.

## Why RAII here specifically

ADR 0051 mandates `panic = "unwind"` and `PatchProcessor::tick` catches module
panics at the tick boundary. With a guard, `Drop` runs during unwind, so FTZ is
restored even when a module panics mid-block. The current bare-OR code leaks the
flipped mode on that path. The guard fixes this as a side effect of correct RAII.

## Acceptance criteria

- [x] `FtzGuard` in `patches-dsp` with `enable()` + `Drop` restore on x86_64
      and aarch64; no-op (still constructs/drops cleanly) on other arches.
- [x] CPAL block handler (`patches-cpal/src/callback.rs::fill_buffer`) holds an
      `FtzGuard` for the block instead of calling `enable_flush_to_zero()`.
- [x] CLAP block handler (`patches-clap/src/plugin.rs::plugin_process`) likewise.
- [x] Guard is bound to a named local (`_ftz_guard`, not `_`), so it is not
      dropped early.
- [x] Drop restores the exact saved MXCSR/FPCR value (unit test
      `enable_sets_ftz_and_drop_restores`).
- [x] Test that restore happens on panic unwind
      (`drop_restores_on_panic_unwind`, via `catch_unwind`).
- [x] `enable_flush_to_zero()` removed (no remaining callers); the
      `patches-engine` re-export now points at `FtzGuard`.
- [x] `just commit` green; determinism tests audited per 0803 still pass.

## Notes

- Epic: tail of E134 (denormal hardening). This closes the "no single owner"
  gap that 0802 + 0805 left open.
- Explicitly out of scope (separate follow-up): deleting the per-site
  `flush_denormal()` calls in biquad / dc_blocker / envelope_follower /
  limiter_core / gate_detector / comp_detector. That deletion is only safe once
  *every* path that runs the kernels establishes FTZ — including the
  integration-test and profiling-bench harnesses, which currently set no FTZ.
  Track that as its own ticket; it changes the E134 regression suite from
  "assert kernel state is normal" to "assert flat CPU under FTZ".
- Future parallel `ExecutionPlan::tick()` (CLAUDE.md desideratum): each worker's
  block loop constructs its own `FtzGuard`. The guard makes per-thread FTZ
  un-forgettable rather than something re-patched with scattered flushes.
