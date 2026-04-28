---
id: "0740"
title: Stereo-aware slot allocation for Tap channels
priority: high
created: 2026-04-27
---

## Summary

Tap channels claim slots by width: mono and trigger channels claim 1,
stereo channels claim 2 consecutive slots (`L`, `R`). The next channel's
`slot_offset` is `prev.slot_offset + prev.width`. `MAX_TAPS` is
reinterpreted as a slot budget and **raised from 32 to 64** (4 backplane
poly slots) so stereo-heavy patches keep usable headroom.

## Acceptance criteria

- [ ] `MAX_TAPS = 64`; backplane reserves 4 consecutive poly slots
      (was 2). Reserved-slot constants and pool initialisation updated.
- [ ] Observer ring widths, manifest arrays, and UI subscriber vectors
      track the new budget without hard-coded `32`s left behind.
- [ ] Planner's slot allocator walks tap channels in order, advancing by
      width.
- [ ] `Tap::tick` for stereo channels writes
      `backplane[slot_offset]   = L`
      `backplane[slot_offset+1] = R`.
- [ ] Manifest's `TapDescriptor` carries `width: u8` (1 or 2); observer
      uses it to size per-slot state.
- [ ] Thirty-two `stereo_meter` taps fit; thirty-three overflow with a
      clear diagnostic.
- [ ] Existing single-slot tap budgets remain correct (no off-by-ones
      in mono-only patches).

## Notes

ADR 0059 §5. The 32→64 bump is the cheap moment: tap allocation is
already being rewritten here, and 0059's slot-not-channel
reinterpretation halves the effective channel count under stereo
metering. Cost is ~512 B working set and a slightly wider observer hot
path; raising later would require manifest-format awareness across
every observer.

Keep `Tap::tick` branchless on width — write two slots unconditionally
for stereo channels (no per-channel `if width == 2`).
