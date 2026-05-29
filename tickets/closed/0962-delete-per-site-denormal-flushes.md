---
id: "0962"
title: Delete per-site flush_denormal() now that FtzGuard owns the invariant
priority: medium
created: 2026-05-28
---

## Summary

0954 made `FtzGuard` the single owner of hardware flush-to-zero at the
real-time block handlers (CPAL, CLAP): FTZ is now a guaranteed property of
running the DSP graph for a block, with correct restore on drop and panic
unwind. That removes the *runtime* justification for the scattered per-site
`flush_denormal()` calls (biquad, dc_blocker, envelope_follower, comp_detector,
gate_detector) — on the audio thread the hardware already flushes subnormal
results, so the per-site branch is dead work.

The per-site flushes only still earn their keep on **offline** paths that run
the same kernels without entering a real-time callback: profiling benches and
integration tests set no FTZ. This ticket closes that gap — establish FTZ on
the offline harnesses first, then delete the per-site flushes and the
`flush_denormal` free fn. The E134 denormal regression then shifts from "assert
the kernel scrubs its own state to normal/zero" (a per-site-flush property) to
"assert flat CPU under an `FtzGuard`" (the property the guard actually owns).

This is structural, not an audio feature. But it is **not** output-neutral on
offline paths (see risk below), so it touches the determinism audit from 0803.

## Call sites to delete (8)

- `patches-dsp/src/dc_blocker.rs:35`
- `patches-dsp/src/envelope_follower.rs:53`
- `patches-dsp/src/biquad/mod.rs:85,86` (mono `tick`)
- `patches-dsp/src/biquad/mod.rs:103,104` (mono saturating path)
- `patches-dsp/src/biquad/mod.rs:248,251` (poly)
- `patches-modules/src/dynamics/common/comp_detector.rs:111`
- `patches-modules/src/dynamics/common/gate_detector.rs:123`

After removal, `patches_dsp::flush_denormal` has no non-test callers — delete
the free fn too (`patches-dsp/src/lib.rs`).

`limiter_core` is named in the 0954 list but has **no** `flush_denormal` call
today: its smoothed gain converges toward `1.0`, never toward subnormal range,
so it needs no flush. Out of scope here.

Three "assert state stays normal over 30 s of silence" unit tests rode on the
per-site flush and must move under an `FtzGuard` (gated to FTZ arches):
`biquad` (`denormal_flush.rs`), `dc_blocker`, `envelope_follower`.

## Offline paths that must establish FTZ first

Real-time paths are done (0954). Remaining kernel-running paths with no FTZ:

- **Profiling benches** — `patches-profiling/src/bin/{bench,fdn_kernel_bench,
  fdn_reverb_bench,limiter_bench,profile}.rs`. Hold an `FtzGuard` for the whole
  `main()` so the bench measures the same FP mode production runs under;
  otherwise the bench reintroduces the subnormal cliff it exists to catch.
  **Done.**
- **Integration tests** — audited (2026-05-28): *no* integration test compares
  bit-exact audio or asserts a CPU/time budget. `soak_randomised_params` checks
  alloc-traps + `Arc` refcounts; the audio-integrity tests assert impulse
  positions / peaks / settling, not byte-equal buffers; determinism tests
  compare two equally-FTZ-free runs (still bit-equal to each other). So deleting
  per-site flushes is **output-safe** for the suite and integration tests do
  **not** need an `FtzGuard`. Adding one would be speculative churn; skipped.
- **Kernel unit tests** — only matter where the test's *point* is denormal
  behaviour (the biquad regression below). Output-correctness unit tests are
  unaffected by subnormals (~-700 dBFS) and need no guard.

## Risk: this is not output-neutral offline

Removing a per-site flush changes offline output: without FTZ, subnormal state
persists as tiny non-zero values where it previously snapped to `0.0`. Adding
`FtzGuard` flushes them again — but hardware FTZ flushes *every* subnormal
result on the thread, not just the specific writes the per-site calls wrapped,
so FTZ-flushed output is **not** bit-identical to the old per-site-flushed
output (and per the E134 epic, not bit-identical across FTZ-incapable CPUs).

Audit before deleting:

- Behavioural integration tests (`fusion_audio_integrity`,
  `auto_conv_audio_integrity`, `hard_sync_aliasing`, …) assert impulse
  positions / peaks / settling, not bit-exact buffers — robust to this.
- Any bit-exact audio golden (incl. feedback-patch goldens flagged in prior
  work) must be re-auditioned and regenerated *under FTZ* if it shifts. Treat
  like the 0803 determinism audit: audition, confirm inaudible, regenerate.

## Acceptance criteria

- [x] Profiling benches hold an `FtzGuard` for `main()`.
- [x] Integration tests audited: none compare bit-exact audio or assert a CPU
      budget, so per-site-flush removal is output-safe and no `FtzGuard` is
      needed in `patches-integration-tests`.
- [x] All 8 per-site `flush_denormal()` calls removed.
- [x] `flush_denormal` free fn removed from `patches-dsp` (no remaining
      non-test callers); no dangling re-exports.
- [x] Three denormal regression tests reframed to hold an `FtzGuard`, gated to
      FTZ arches: `biquad` (`denormal_flush.rs`), `dc_blocker`,
      `envelope_follower`. The biquad one is made *discriminating* — it locates
      a sample where the unguarded state is a nonzero subnormal and proves the
      guarded run is exactly `0.0` there (a pure value assertion would not
      distinguish FTZ from gradual underflow). The two follower tests assert no
      output is subnormal at any sample across 30 s of silence.
- [x] Determinism / golden audit per 0803: repo has **no** bit-exact audio
      golden fed by these kernels (`.wav` files at root are stray render output,
      not test fixtures; `.snap` snapshots are SVG/LSP text). Behavioural audio
      tests (`fusion_audio_integrity`, `auto_conv_audio_integrity`,
      `hard_sync_aliasing`) pass unchanged. Nothing to regenerate.
- [x] `just commit` green (patches-dsp, patches-modules,
      patches-integration-tests, patches-profiling).
- [x] `just push` green (full suite incl. integration/clap/lsp; exit 0).

## Notes

- Epic: tail of E134 (denormal hardening), continuing 0954. Closes the
  "no single owner" gap for offline paths that 0802 + 0805 left to per-site
  flushes.
- Future parallel `ExecutionPlan::tick()` (CLAUDE.md desideratum): each worker
  block loop already constructs its own `FtzGuard` (0954), so per-worker FTZ is
  un-forgettable without the scattered flushes this ticket removes.
- `sanitize()` (NaN/Inf guard) is an orthogonal concern and stays.
