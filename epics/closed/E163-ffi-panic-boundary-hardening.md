---
id: E163
title: FFI / panic-boundary hardening
status: closed
created: 2026-06-11
closed: 2026-06-11
---

## Goal

Close the three critical safety gaps found in the 2026-06 holistic review,
all sharing one theme: **the ADR 0051 panic fence is narrower than the
audio-callback boundary, and the plugin FFI surface has no fence at all.**
A panicking third-party module, a planner invariant failure during plan
adoption, or a `FfiBytes` round-trip can today abort the host DAW (or worse,
UB) instead of producing the clean halt ADR 0051 promises.

Fix before the FFI surface is exposed to third-party plugin authors.

## Background

- `PatchProcessor::tick` wraps module processing in `catch_unwind`
  (ADR 0051), but `adopt_plan_with_meta` runs on the same audio callback
  *before* tick, outside the fence, and contains a hard `expect`
  (`patches-engine/src/processor.rs:332`). Neither
  `patches-cpal/src/callback.rs` nor `patches-clap/src/plugin.rs` wraps the
  callback itself.
- The `export_plugin!` / `export_modules!` macros generate `extern "C"`
  entry points that call user `Module` code bare — no `catch_unwind`
  anywhere in `patches-ffi-common/src/sdk.rs`. Since Rust 1.81, unwinding
  out of `extern "C"` is a guaranteed **process abort**; the host-side
  fence never sees it.
- `FfiBytes::from_vec` discards `Vec` capacity;
  `reclaim` rebuilds with `Vec::from_raw_parts(ptr, len, len)`
  (`patches-ffi-common/src/types.rs:59-91`). Dealloc with the wrong
  `Layout` is genuine UB per the `GlobalAlloc` contract.

## Scope

**In:**

- 0994 — `FfiBytes` carries capacity (ABI-breaking field add, done while
  the ABI is still internal).
- 0995 — `catch_unwind` fence in every generated plugin entry point;
  sentinel error path; halt propagation to the host.
- 0996 — widen the host-side fence to the whole audio callback (plan
  adoption included) in both patches-cpal and patches-clap; reconcile the
  `debug_assert` / hard-`expect` asymmetry inside `adopt_plan_with_meta`.
- ADR 0051 amendment: define the fence as *the entire audio callback
  including plan adoption*, with the SDK macros as the plugin-side fence.

**Out:**

- General `unwrap`/`expect` policy sweep (ticket 1000).
- RT allocation/I-O hygiene (ticket 0997).

## Tickets

- [x] 0994-ffibytes-capacity-round-trip
- [x] 0995-sdk-macro-catch-unwind-fence
- [x] 0996-widen-audio-callback-panic-fence

All three closed 2026-06-11. ABI bumped 12 → 13 (`FfiBytes.cap` field +
audio-thread entry `-> i32` panic sentinels). ADR 0051 amended with the
two-layer fence (host callback fence + plugin SDK fence). New
`test-panic-plugin` fixture + two integration tests
(`ffi_panic_halt::panic_in_dylib_process_halts_cleanly`,
`panic_halt::panic_during_plan_adoption_halts_cleanly`).

## Notes

Source: 2026-06 holistic review (13-agent crate sweep, lead-verified).
All three findings hand-verified against source.
