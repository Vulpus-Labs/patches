---
id: "0837"
title: Typed enums for CLAP scope/spectrum mode round-trips
priority: low
created: 2026-05-08
epic: E139
---

## Summary

`patches-clap` state serialization stores `scope_snap` and
`spectrum_heatmap` as `bool` in memory but writes them as `u32` (0/1) on
the wire, then converts back at load time. The repeated `if x { 1 } else { 0 }`
shuffle is primitive obsession: the underlying concept is "which mode is
this view in", and the wire format would be the same width if it were a
proper enum from the start.

## Sites

- [patches-clap/src/extensions.rs:240-243](../../patches-clap/src/extensions.rs#L240)
  — bool→u32 serialization shuffle.
- [patches-clap/src/extensions.rs:319-369](../../patches-clap/src/extensions.rs#L319)
  — deserialization match ladder (`ReadU32::Ok/Eof/Err` per field).

## Proposed shape

```rust
#[repr(u32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum ScopeMode { Free = 0, Snap = 1 }

#[repr(u32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum SpectrumRender { Curves = 0, Heatmap = 1 }

impl ScopeMode {
    fn from_wire(v: u32) -> Self { if v == 0 { Self::Free } else { Self::Snap } }
    fn to_wire(self) -> u32 { self as u32 }
}
```

Backward compat: `from_wire` must accept 0/1 with no other panic path so
existing saved state continues to load. Unknown values clamp to the
default rather than erroring (state is best-effort restore).

## Acceptance criteria

- [ ] `scope_snap: bool` and `spectrum_heatmap: bool` replaced by typed
      enums in the controller / view-state structs
- [ ] Wire format unchanged (still 0/1 as `u32`)
- [ ] State written by the previous version loads without warning
- [ ] Match ladder in deserialization is shorter (one helper handles
      "read u32, default-on-eof, propagate-on-err")
- [ ] `just commit -p patches-clap` clean

## Notes

Worth doing only because the same pattern will recur as more view modes
are added (FFT window, scope trigger, etc.). Solo this ticket the enum
infrastructure can be reused; left as bools, every new toggle reinvents
the round-trip.

Out of scope: changing the wire format width. Future format-version bump
is a different ticket.
