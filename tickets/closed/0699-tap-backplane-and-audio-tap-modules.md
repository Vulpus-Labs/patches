---
id: "0699"
title: Backplane + AudioTap/TriggerTap audio-side modules
priority: high
created: 2026-04-26
---

## Summary

Implement the backplane (`[f32; MAX_TAPS]`, `MAX_TAPS = 32` per ADR
0053 §4) and the audio-side `AudioTap` / `TriggerTap` module
implementations per ADR 0054 §4. Per-tick action per channel:
`backplane[slot_offset[i]] = inputs[i]`. No allocation, no branching
on tap type, no per-channel reduction. The desugared synthetic module
instances from E118 §0697 are the only consumers of this code path.

## Acceptance criteria

- [ ] Backplane lives in the engine alongside the per-tick state, sized
  to `MAX_TAPS` const exported from a shared location (so observer and
  ring sizing reference the same constant).
- [ ] `AudioTap` and `TriggerTap` register as ordinary modules with
  `channels` shape param and `tap_name[i]: str` + `slot_offset[i]: int`
  per-channel params (matching what the desugarer emits).
- [ ] `tick`: per channel, write `backplane[slot_offset[i]] = inputs[i]`.
  Mono input cables only; types enforced by existing `MonoLayout::Audio`
  / `MonoLayout::Trigger` cable-type checking.
- [ ] No allocation on the audio thread. No mutex, no syscall.
- [ ] Trigger cables write the cable's native sub-sample encoding
  (ADR 0047) — no edge detect, no decoding. Observer reconstructs.
- [ ] Unit tests: hand-built module instance, drive inputs, assert
  backplane state per tick. Confirm `slot_offset` values from the
  param map are honoured.
- [ ] `cargo test -p patches-modules -p patches-engine` green.

## Notes

The frame ring (ticket 0700) and observer (ticket 0701) consume the
backplane; this ticket can be tested in isolation by inspecting the
backplane directly after `tick()`.

ADR 0054's per-tick formula `backplane[slot_offset + i]` reads as
"one offset, channel-relative" but the descriptor gives a per-channel
`slot_offset[i]`. Implement the per-channel form; it generalises to
non-contiguous global slot orderings (interleaved audio/trigger taps).

## Close-out notes

- Backplane lives on `PatchProcessor` as `tap_backplane: TapFrame`;
  reachable from modules via `CablePool::write_backplane`. `MAX_TAPS = 32`
  and `TapFrame = [f32; MAX_TAPS]` are exported from `patches-core` so
  observer and ring share one source of truth.
- Modules: `AudioTap` (mono inputs) and `TriggerTap` (trigger inputs).
  Both declare `channels` shape and `slot_offset[i]: int` per channel.
- **Deviation from acceptance criteria:** `tap_name[i]: str` was dropped.
  No `ParameterKind::String` exists in the parameter system, and the
  audio thread has no use for the names — they live in the observer
  manifest only. The desugarer was updated to omit `tap_name`. Re-add
  iff a future need surfaces audio-side.
- `ModuleHarness::enable_backplane()` / `backplane()` lets module unit
  tests inspect backplane writes without standing up an engine.

## Cross-references

- ADR 0053 §4 — `MAX_TAPS`.
- ADR 0054 §4 — module decomposition.
- E118 ticket 0697 — desugarer that emits these module instances.
- E119 — parent epic.
