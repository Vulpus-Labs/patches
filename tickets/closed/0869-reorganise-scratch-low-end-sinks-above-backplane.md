---
id: "0869"
title: Reorganise scratch low-end so sinks live above backplane
priority: medium
created: 2026-05-11
epic: E145
---

## Summary

Today's scratch index space ([patches-core/src/cables/mod.rs:71-74](patches-core/src/cables/mod.rs#L71-L74)):

```text
[0, SINK_SLOTS)              sinks            (4 slots: mono/poly read/write)
[SINK_SLOTS, RESERVED_SLOTS) backplane        (audio I/O, transport, MIDI, tap, host-control)
[RESERVED_SLOTS, …)          dyn (planner-allocated)
```

Reorganise to:

```text
[0, BACKPLANE_SIZE)                         backplane         (host-only, plugin-invisible after 0870)
[BACKPLANE_SIZE, BACKPLANE_SIZE + 4)        sinks             (mono read, poly read, mono write, poly write)
[RESERVED_SLOTS, …)                         dyn
```

This is the precondition for 0870 (cutting plugin scratch base past
the backplane). Sinks must sit above the backplane line so that
plugin-relative indices `[0, 4)` resolve to sink slots after the
shift, leaving disconnected port wiring unchanged from the plugin's
view.

`RESERVED_SLOTS` and `SCRATCH_CAPACITY` are unchanged. Sink and
backplane symbols (`MONO_READ_SINK`, `POLY_READ_SINK`,
`MONO_WRITE_SINK`, `POLY_WRITE_SINK`, `AUDIO_OUT_L`, …) keep their
names; only their numeric values shift. All in-tree code addresses
through the symbols, so the change is internal.

## Acceptance criteria

- [ ] In [patches-core/src/cables/mod.rs](patches-core/src/cables/mod.rs):
      backplane constants moved to start at index 0 (`AUDIO_OUT_L = 0`,
      `AUDIO_OUT_R = 1`, …); sink constants moved to land just below
      `RESERVED_SLOTS`; doc comment block at lines 70-75 updated to
      reflect the new layout.
- [ ] [patches-engine/src/kernel.rs:39-40](patches-engine/src/kernel.rs#L39-L40)
      sink init still touches the right slots (uses symbols, no fix
      needed in code; verify the engine still zero-fills the new
      sink positions).
- [ ] Audit `patches-engine` and `patches-modules` for any raw
      arithmetic on `SINK_SLOTS` / `RESERVED_SLOTS` that assumes
      sinks-then-backplane ordering (e.g. `RESERVED_SLOTS - SINK_SLOTS`
      meaning "end of backplane"). Fix any such site.
- [ ] ABI version bumped to v12 in
      [patches-ffi-common/src/types.rs](patches-ffi-common/src/types.rs)
      with a doc comment explaining the reason (backplane reorg;
      0870 will follow with the plugin-visibility cut).
- [ ] Existing FFI plugin tests (`test-plugins/`) still pass after
      rebuild — they should be unaffected since none of them
      address backplane slots, but verify against the new ABI.
- [ ] `just push` clean.

## Notes

This is part of ADR 0072's cable-pool layout work even though it
isn't strictly part of the original phase plan. Consider adding a
phase 6 note to ADR 0072 if the change becomes contentious;
otherwise the change is small enough to land without a new ADR.

The sink slots themselves remain harmless when plugin-visible
(0870): read sinks are always-zero and never written; write sinks
are never read (cables/mod.rs:79-101). A misbehaving plugin
scribbling in `MONO_WRITE_SINK` corrupts nothing.
