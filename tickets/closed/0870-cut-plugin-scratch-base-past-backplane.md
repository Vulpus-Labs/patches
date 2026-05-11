---
id: "0870"
title: Cut plugin scratch base past backplane (forbid backplane addressing from FFI plugins)
priority: medium
created: 2026-05-11
epic: E145
depends-on: "0869"
---

## Summary

After 0869 the scratch layout is `[backplane | sinks | dyn]`. This
ticket makes the backplane region invisible to FFI plugins.

The host loader passes plugin a *shifted* scratch view that begins
at `BACKPLANE_SIZE`:

- [patches-ffi/src/loader.rs](patches-ffi/src/loader.rs) `process` and
  `periodic_update` dispatch use
  `scratch_ptr.add(BACKPLANE_SIZE)` and `scratch_len - BACKPLANE_SIZE`
  for the plugin-visible slice. Cycle pool is unchanged (no backplane
  there).
- The planner translates `cable_idx` for any port crossing the FFI
  boundary: scratch-region indices have `BACKPLANE_SIZE` subtracted
  before being packed into `FfiInputPort` / `FfiOutputPort`
  ([patches-ffi-common/src/port_frame.rs:144](patches-ffi-common/src/port_frame.rs#L144)).
  Cycle indices are untranslated. Sinks (engine indices
  `[BACKPLANE_SIZE, BACKPLANE_SIZE + 4)`) translate to plugin-relative
  `[0, 4)` — disconnected port wiring keeps working without code
  change in the plugin SDK.
- The planner refuses to wire any FFI plugin port to a backplane
  slot. (Audit needed — likely already true since backplane
  producers/consumers are internal modules; explicit assertion is
  the deliverable.)

After this lands, future backplane reorgs (adding new global slots,
shifting `AUDIO_OUT_L`, growing tap/host-control regions) no longer
force ABI bumps. The whole "backplane constants leaked to plugins"
class of bumps is dead.

## Acceptance criteria

- [ ] Loader passes shifted `scratch_ptr` / `scratch_len` to plugin
      `process` and `periodic_update`. Cycle pool args unchanged.
- [ ] `pack_ports_into`
      ([patches-ffi-common/src/port_frame.rs:144](patches-ffi-common/src/port_frame.rs#L144))
      accepts a `scratch_base_offset: usize` parameter (or the
      planner translates indices upstream and `pack_ports_into`
      stays untouched — pick one). Scratch-region indices in
      `inputs`/`outputs` get the offset subtracted before being
      stored in `FfiInputPort.cable_idx` / `FfiOutputPort.cable_idx`.
      Cycle indices pass through unchanged.
- [ ] Translation is the planner's responsibility, not the plugin
      SDK's; the plugin reconstructs `CablePool::new(scratch, cycle, wi)`
      and addresses cables via the same plugin-relative indices it
      received.
- [ ] Planner asserts no FFI plugin port resolves to a backplane
      cable_idx. Violation is a `BuildError` at planning time, not a
      silent runtime corruption. Add a unit test in
      [patches-engine](patches-engine) or
      [patches-interpreter](patches-interpreter) covering the rejection.
- [ ] `BACKPLANE_SIZE` exposed as a `pub const` from `patches-core::cables`
      (not `RESERVED_SLOTS - SINK_SLOTS` in two places).
- [ ] Existing test plugins in `test-plugins/` continue to work after
      rebuild against ABI v12.
- [ ] Add a test plugin (or extend an existing one) that exercises
      a disconnected mono input + disconnected poly output, to
      regression-guard the sink-translation path.
- [ ] ABI version bumped (v12 from 0869 covers both changes if they
      land in the same push; otherwise bump again).
- [ ] `just push` clean.

## Notes

The planner refusal to wire backplane-bound ports from FFI plugins
is the only behavioural restriction this introduces. In practice it
means an external plugin cannot *be* the master output sink — the
user must wire `plugin_out → AudioOut`, where `AudioOut` is an
in-tree module. This matches CLAP/VST/AU semantics and isn't worth
the ABI fragility cost of allowing the alternative.

Index width: cable_idx is `usize` today. After translation it stays
`usize`. Assert at prepare time that `BACKPLANE_SIZE < scratch_len`
so subtraction can't underflow on a degenerate config.

Once this lands the SDK contract reduces to:

> Plugin scratch indices `[0, 4)` are reserved sinks (mono read,
> poly read, mono write, poly write). Plugin scratch indices `[4, …)`
> are dynamic cables assigned by the planner. Cycle indices `[0, …)`
> are dynamic cables.

— two lines, no mention of backplane.
