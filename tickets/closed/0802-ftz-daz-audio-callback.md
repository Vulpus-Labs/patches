---
id: "0802"
title: Set FTZ/DAZ on audio callback entry
priority: high
created: 2026-05-04
---

## Summary

Enable flush-to-zero / denormals-as-zero in the audio callback for
patches-engine (CPAL) and patches-clap (host process callback).
Eliminates the denormal CPU cliff globally for the DSP graph at the
cost of one register write per buffer. See E134 for context and
tradeoffs.

## Acceptance criteria

- [ ] x86_64: set MXCSR FTZ (bit 15) + DAZ (bit 6) on callback entry.
- [ ] aarch64: set FPCR.FZ (bit 24) on callback entry.
- [ ] Set per callback, not once at startup (hosts may reset MXCSR
      between callbacks).
- [ ] CPAL stream callback in patches-engine wrapped.
- [ ] CLAP `process` entry in patches-clap wrapped.
- [ ] Helper lives in a single place (patches-engine or
      patches-core) — no duplicated arch cfg blocks.
- [ ] Other architectures (riscv, etc.): no-op, no compile error.
- [ ] No new dependencies if avoidable; if `no_denormals` crate is
      adopted, ask first per CLAUDE.md.

## Notes

- Per-thread state. If/when `ExecutionPlan::tick()` parallelises,
  worker threads need the same setup on their entry point.
- Use raw intrinsics (`_mm_setcsr` / inline asm `MSR FPCR`) or a
  small RAII wrapper; either is fine.
- Don't restore prior MXCSR on exit — the audio thread is ours.
