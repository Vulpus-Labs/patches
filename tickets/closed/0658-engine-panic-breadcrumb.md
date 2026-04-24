---
id: "0658"
title: ExecutionPlan attribution breadcrumb and halt flag
priority: high
created: 2026-04-24
epic: E113
adr: 0051
---

## Summary

Add the state `ExecutionPlan` needs to identify which module was ticking
when a panic fires, and to latch the plan into a silent halted state.
No `catch_unwind` yet — that arrives in 0659.

## Acceptance criteria

- [ ] `ExecutionPlan` gains `current_module_slot: AtomicUsize` initialised
      to `usize::MAX`.
- [ ] Before each `module.process()` call, the slot index is stored with
      `Relaxed`; after the call, `usize::MAX` is restored.
- [ ] Same bracketing around `periodic_update()` calls.
- [ ] `ExecutionPlan` gains `halted: AtomicBool` initialised `false` and
      `HaltInfo { slot: usize, module_name: &'static str, payload: String }`
      in a `Mutex` or equivalent non-RT-facing cell. Writing is from the
      audio thread post-catch; reading is control-thread only.
- [ ] No behavioural change in the happy path: with no panic, `tick()`
      still runs every module and produces identical output to before.
- [ ] Bench: one relaxed store per module per sample; confirm no
      measurable regression on the existing `patches-profiling` baseline.

## Notes

Writing `halt_info` from inside `catch_unwind`'s `Err` branch is not on
the audio-thread hot path and can allocate — the unwind already
allocated the panic payload. Keep the field layout simple.
