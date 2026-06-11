---
id: "0996"
title: Widen panic fence to whole audio callback (plan adoption included)
priority: high
created: 2026-06-11
---

## Summary

The ADR 0051 `catch_unwind` covers `PatchProcessor::tick` only. Plan
adoption runs on the same audio callback *before* tick —
`receive_plan` at `patches-cpal/src/callback.rs:198` and
`try_adopt_pending` in `patches-clap/src/plugin.rs` — and
`adopt_plan_with_meta` contains a hard
`expect("adopt_plan: param_frames shorter than parameter_updates (planner bug)")`
(`patches-engine/src/processor.rs:332`). A planner invariant failure
unwinds uncaught through the CPAL closure / CLAP `extern "C"` frame:
abort, not clean halt.

Same function is internally inconsistent: lines 310 and 325 check the same
invariant class with `debug_assert_eq!` (silently dropped in release)
while line 332 is a live release panic.

## Acceptance criteria

- [x] Panic fence covers the entire audio callback in both
      `patches-cpal/src/callback.rs` (`fill_buffer`) and `patches-clap`
      `plugin_process` (adoption + tick + record/MIDI paths), feeding the
      ADR 0051 halt state via `PatchProcessor::record_callback_halt` on Err.
- [x] `adopt_plan_with_meta` invariant checks made consistent: the bare
      release `expect` at the `param_frames` shortfall is now a
      `debug_assert!` that breaks (skips remaining updates) in release,
      matching the neighbouring `debug_assert_eq!`s. No bare release
      `expect` remains.
- [x] `DylibModule::set_ports` `expect` covered: it runs inside
      `adopt_plan_with_meta`, now under the widened callback fence.
- [x] Integration test
      `panic_halt::panic_during_plan_adoption_halts_cleanly`: an
      out-of-bounds-`to_zero` malformed plan panics during adoption and is
      recorded as a clean halt (slot `NO_SLOT`), no abort, halt sticky.
- [x] ADR 0051 amendment documents the fence boundary = whole callback
      (E163 amendment, point 1).

## Resolution

`record_callback_halt` + `is_halted` added to `PatchProcessor`; both hosts
wrap their callback body in `catch_unwind`. The malformed-`param_frames`
`expect` downgraded to a debug assert. `HeadlessEngine::adopt_plan_guarded`
added so the widened fence is testable headless. Closed under **E163**.

## Notes

Part of **E163**. Mind the fence cost: `catch_unwind` is zero-cost on the
non-panic path with unwind tables (panic = "unwind" already mandated by
ADR 0051), so one outer fence per callback is fine; do not fence
per-module (tick already does that).
