---
id: "0935"
title: Tap::process only writes active backplane slots
priority: medium
created: 2026-05-19
---

## Summary

`Tap::process` runs every audio tick and unconditionally writes all four
`TAP_SLOTS` poly frames into the backplane scratch region, regardless of
how many tap channels the patch actually has. With `MAX_TAPS = 64` and
`TAP_SLOTS = 4`, that is ~768 B of writes per sample (256 B stack
zero-init + 4×64 B memcpy + 4×64 B `CableValue::poly` store) — ~37 MB/s
at 48 kHz. Most patches use slot 0 only; 75% of the work is wasted.
This shows up as ~0.7% CPU in monitoring even on light patches.

Cache an "active slots" plan in `update_validated_parameters` and only
emit those slots in `process`. Per active slot, build the 16-wide frame
directly from the channels that map into it — no `[f32; MAX_TAPS]`
scratch buffer.

## Acceptance criteria

- [x] `Tap` caches a per-slot channel plan when params update.
- [x] `process` skips slots with no wired channels.
- [x] Disconnected lanes within an active slot still write `0.0`.
- [x] Existing tap tests in `patches-modules/src/tap.rs` pass unchanged.
- [x] `just inner -p patches-modules` clean.

## Notes

- `Tap` writes `PolyOutput::backplane(TAP_BASE + i)` per slot; calling
  this every tick is fine (it's a 2-field struct literal), no need to
  cache the `PolyOutput` itself.
- Out-of-range `slot_offset` handling is preserved (the planner keeps
  them in range, but a stale plan crossing a region shrink must not
  corrupt other backplane state).
- Stereo lanes spanning a slot boundary (L in slot `n`, R in slot `n+1`)
  must mark both slots active.
- No change to descriptor, port wiring, or backplane layout — purely
  internal to `Tap::process` / `update_validated_parameters`.

## Outcome

Measured CPU dropped from ~0.7% to ~0.5-0.6% on a light patch. Matches
removing 3 of 4 frame writes per sample. Remaining tap cost is the
single active-slot frame write + per-channel cable reads + dyn-dispatch
`Module::process` — the floor for any module that touches the pool.
