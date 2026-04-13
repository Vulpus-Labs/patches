---
id: "0396"
title: Pass 2 high — remove eprintln from audio thread, strengthen transmute doc, structured plugin error
priority: high
created: 2026-04-13
---

## Summary

Code review pass 2 surfaced three high-severity items on the audio thread and
the real-time ↔ non-real-time boundary:

1. `eprintln!` on the audio thread during plan adoption when the cleanup ring
   buffer is full (`patches-engine/src/processor.rs`).
2. A `transmute` that erases the lifetime on a `PeriodicUpdate` trait pointer
   in `patches-engine/src/pool.rs`. Safety argument is sound but the
   precondition (what `as_periodic()` may return) is not documented on the
   trait.
3. `patches-clap/src/plugin.rs` stringifies DSL / interpreter / planner errors
   via `.to_string()` before pushing them across the compile path, losing
   structured error information.

## Acceptance criteria

### H1 / H2 — No `eprintln!` on the audio thread

- [x] `eprintln!` removed from both cleanup-ring overflow paths in
      `processor.rs`. Inline `drop(action)` retained as fallback.
- [x] Added `cleanup_overflow_count: AtomicU32` on `PatchProcessor` with
      `Relaxed` ordering; exposed via `cleanup_overflow_count(&self) -> u32`
      for non-RT polling.

### H4 — Document `as_periodic()` precondition on the Module trait

- [x] Expanded SAFETY comment in `pool.rs::as_periodic_ptr` to three
      numbered clauses covering module lifetime, the "must reference
      module-owned data" precondition, and the rebuild-before-tick invariant.
- [x] Added `# Safety contract` section to `Module::as_periodic` in
      `patches-core/src/modules/module.rs` naming the precondition.

### H5 — Structured compile-path error for the CLAP plugin

- [x] `patches-clap/src/error.rs` defines `CompileError` with variants
      `NotActivated`, `Load`, `Parse`, `Expand`, `Interpret`, `Plan` and
      `From` impls for each source error type. Variants carry
      `BuildError` (which already wraps `PlanError`) rather than the
      planner's internal `PlanError` directly.
- [x] `compile_and_push_plan` / `load_or_parse` return
      `Result<_, CompileError>`; all `.to_string()` / `map_err(|e|
      e.to_string())` bridges removed.
- [x] Call sites continue to consume via `Display`; no reporting
      boundary change required.

## Notes

Pass-2 review, findings H1/H2/H4/H5.

H3 (Arc::clone per receiver) was withdrawn: `Arc::clone` is a wait-free
atomic fetch-add with no allocation, so the per-receiver loop is fine.

Not in scope: FFI / CLAP host-boundary error codes. `CompileError` only
replaces internal string-error plumbing inside the plugin crate.
