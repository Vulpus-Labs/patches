# ADR 0051 — Module panic halt policy

**Date:** 2026-04-24
**Status:** accepted

---

## Context

With ADR 0045's FFI plugin path live, the audio thread now invokes module
code that was compiled outside the host binary. A panic in any such module
propagates as a Rust unwind through the `extern "C"` boundary into the host
(DAW or `patches-player`), which is undefined behaviour in the worst case
and a host crash in the common one. Native in-process modules carry the
same hazard in principle, though panics there are always a Patches bug.

The execution model precludes the usual mitigation of "skip the failing
module and continue": every module ticks together sample-by-sample against
a shared `CablePool` with a one-sample cable delay. Omitting a module from
the main signal path silences any output that depends on it; omitting one
off-path still leaves the graph in an unexplained partial-truth state.
There is no useful "degraded" mode.

Rescan and patch reload already rebuild the entire engine context from
scratch (ADR 0038), so recovery does not need to be incremental either.

## Decision

A panic from any module's `process()` or `periodic_update()` halts the
engine cleanly: the audio thread catches the unwind, identifies the
offending module, marks the processor halted, and returns silence on every
subsequent tick until the host triggers a rebuild.

Concretely:

1. **Attribution breadcrumb.** `ExecutionPlan` holds an
   `AtomicUsize` (`current_module_slot`) that is stored with the slot
   index before each `module.process()` / `periodic_update()` call and
   cleared after. One relaxed store/load per module per sample.

2. **Tick-level catch.** `ExecutionPlan::tick()` wraps its body in
   `std::panic::catch_unwind(AssertUnwindSafe(...))`. On `Err`, it reads
   the breadcrumb, writes the slot index and a one-shot panic payload
   summary to a `HaltInfo` field, sets `AtomicBool::halted`, zeroes the
   tick's output buffers, and returns.

3. **Halt is sticky.** Once halted, `tick()` short-circuits to silence
   without entering the module loop. Only a full plan rebuild clears the
   halt flag; there is no in-place reset.

4. **Surfacing.** `Processor` exposes a non-blocking `halt_info()` query
   readable from the control thread. `patches-player` prints a diagnostic
   naming the halted module and waits for user action (reload patch).
   `patches-clap` sets a GUI error banner naming the module and keeps
   feeding silence to the host until the user triggers a rescan or patch
   reload.

5. **`AssertUnwindSafe` is justified.** A halted plan is never re-entered,
   so observers cannot witness the torn state left by a mid-process
   unwind. The assertion documents that invariant at the call site.

## Consequences

**Positive:**

- Host process survives any module panic. FFI plugins become safe to load
  without trusting their authors.
- A single mechanism covers both native and FFI panics. No per-path
  bespoke handling.
- Attribution is cheap: one `AtomicUsize` store per module tick, no extra
  landing pads per module.
- Recovery path is the existing rebuild path; nothing new to design.

**Negative:**

- A deterministic panic forces the user to notice and reload. No automatic
  retry. Acceptable given the "rebuild = user action" stance.
- `catch_unwind` is not free on every audio-thread target. On tier-1
  Unix-like targets with table-based unwinding the happy-path cost is a
  few nanoseconds per sample, below noise; on `panic = "abort"` builds
  the wrapper is a no-op and halt-on-panic becomes halt-on-abort
  (i.e. process exit). Plugin and host crates must keep
  `panic = "unwind"` for the policy to function.
- Mid-tick state after a panic is torn. Because halt is sticky, no
  observer ever reads that state, but anything held across ticks (ring
  buffers, cleanup-thread queues) must still be valid enough for the
  host process to unload the plan cleanly.

## Alternatives considered

- **Per-module `catch_unwind`.** Gives per-module attribution without a
  breadcrumb and allows quarantining the offending module while running
  the rest of the graph. Rejected: there is no sensible "run without
  this module" state in the current execution model, and wrapping every
  module adds an unwind frame per module per sample.
- **`panic = "abort"` in plugins.** Defined behaviour across FFI but kills
  the host process. No recovery, no diagnostic. Unacceptable for CLAP.
- **Automatic rebuild on panic.** A deterministically-panicking module
  would trigger a rebuild loop at sample rate. Rejected; recovery is a
  user action.

---

## Amendment — 2026-06-11 (E163): fence boundary = the whole audio callback

The original decision fenced `PatchProcessor::tick` only, and assumed the
unwind would reach the host-side `catch_unwind`. Two gaps surfaced in the
2026-06 holistic review:

1. **Plan adoption runs outside the tick fence.** `adopt_plan_with_meta`
   executes on the same audio callback *before* `tick` — it installs
   modules, calls their `set_ports`, and drops tombstoned modules. A panic
   there (a panicking plugin `Drop`/`set_ports`, or a planner-invariant
   failure) unwound uncaught through the CPAL closure / CLAP `extern "C"`
   frame: abort, not clean halt.

2. **An unwind never crosses an `extern "C"` boundary.** Since Rust 1.81 an
   unwind escaping an `extern "C"` function is a guaranteed process abort.
   A panic inside a plugin's own `process` aborts in the *plugin's* frame,
   before control returns to the host — so the host-side `tick` fence can
   never see it. The plugin needs its own fence.

The fence boundary is therefore redefined as **the entire audio callback**,
with two cooperating layers:

1. **Host-side callback fence (ticket 0996).** `patches-cpal` and
   `patches-clap` wrap their whole audio callback — plan adoption, MIDI
   ingest, the tick loop, and the record/meter flush — in
   `catch_unwind`. On `Err` they call
   `PatchProcessor::record_callback_halt`, which records a sticky halt
   against `NO_SLOT` (there is no per-module breadcrumb during adoption).
   The inner `tick` fence (point 2 of the original decision) is retained,
   nested, for per-module attribution; the outer fence is the backstop for
   everything else. Both are zero-cost on the happy path with unwind
   tables. `adopt_plan_with_meta`'s invariant checks are now uniform
   `debug_assert`s (no bare release `expect`), degrading to skipped work in
   release under the fence.

2. **Plugin-side SDK fence (ticket 0995).** The `export_plugin!` /
   `export_modules!` / `export_plugin_with_hash_override!` macros route every
   user-code call (`process`, `set_ports`, `update_validated_parameters`,
   `periodic_update`, `prepare`, `template`) through
   `patches_ffi_common::sdk::fence`, a `catch_unwind` inside the plugin.
   The audio-thread entry points return `FFI_ENTRY_OK` / `FFI_ENTRY_PANIC`
   (ABI v13); on `FFI_ENTRY_PANIC` the host loader re-raises a panic on the
   host side of the boundary (`DylibModule::ffi_panic`), where the tick /
   callback fence above turns it into a clean halt. `prepare` panics are
   surfaced as a `PREPARE_ERR_PREPARE` build error instead. This is the
   per-path fence the original ADR rejected for *native* modules — but FFI
   leaves no choice, since the abort happens before the host regains
   control.
