---
id: "0707"
title: Observer/TUI hardening — OOB diagnostic, manifest generation, name-keyed drop baselines
priority: high
created: 2026-04-26
---

## Summary

Three correctness fixes from the E119 review:

1. **OOB slot diagnostic.** `Observer::apply_manifest` silently
   `continue`s past `TapDescriptor`s with `slot >= MAX_TAPS`. A
   malformed manifest entry vanishes without trace. Emit a
   diagnostic on the existing diagnostics channel.
2. **Manifest generation on tap frames.** When the planner ships a
   new manifest, frames already in the SPSC ring still carry
   old-slot semantics. The observer swaps slot wiring on receipt
   of the new manifest; in-flight frames are then misinterpreted
   for one drain cycle. Stamp a `manifest_generation: u32` on
   `PatchProcessor` (incremented on every manifest swap), copy it
   into `TapBlockFrame`, drop frames whose generation does not
   match the observer's current generation.
3. **Drop baselines keyed by tap name.** TUI's `View.drop_seen` /
   `drop_logged_at` are keyed by slot. A tap removed and re-added
   under the same slot inherits a stale baseline; a tap renamed
   into a new slot loses its baseline (false "drop detected"
   spam). Key both maps by tap name; `set_taps` retains entries
   for surviving names.

## Acceptance criteria

- [ ] `patches-observation::observer::apply_manifest` emits a
  diagnostic (new variant `Diagnostic::InvalidSlot { slot,
  tap_name }`) when a `TapDescriptor` has `slot >= MAX_TAPS`.
- [ ] `TapBlockFrame` (in `patches-core`) gains a
  `manifest_generation: u32` field. Default `0`. Cost: 4 B/frame.
- [ ] `PatchProcessor` has a `tap_manifest_generation: u32`,
  bumped (`wrapping_add(1)`) on each call to a new
  `set_tap_manifest_generation` method (or wired into the existing
  manifest-installation path). Stamped onto every emitted block
  frame.
- [ ] `Observer` tracks its own `current_generation: u32`. On
  manifest receipt, generation is updated; subsequent frames with
  mismatched generation are dropped *without* incrementing the
  per-slot drop counters (drop-counter semantics are reserved for
  ring-full overflow, not generation skew).
- [ ] `View.drop_seen` and `View.drop_logged_at` are keyed by
  `String` (tap name). `set_taps` retains entries by name.
- [ ] Tests:
  - Observer emits `InvalidSlot` diagnostic for OOB descriptors.
  - Frames with stale `manifest_generation` are silently dropped
    by the observer (no panic, no slot writes, no drop counter
    bump).
  - TUI test: tap removed then re-added under same slot does not
    produce a false drop log line; tap renamed under same slot
    treats baseline as fresh.
- [ ] `cargo clippy` clean; `cargo test -p patches-core -p
  patches-modules -p patches-engine -p patches-observation -p
  patches-player` green.

## Notes

- Generation is a `u32`; wraparound at 2^32 manifest swaps is
  effectively never. No need for u64.
- `Diagnostic::InvalidSlot` is a hard error from the observer's
  perspective (the tap is unobservable); upstream planner should
  prevent it. The diagnostic surfaces planner bugs during
  development without crashing the audio path.
- See E119 epic and 0706 (sample_time retrofit) for context.
