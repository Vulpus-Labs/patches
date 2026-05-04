---
id: "0809"
title: HostControl module and backplane region
priority: high
created: 2026-05-04
epic: E135
depends_on: "0808"
---

## Summary

Implement the `HostControl` module (ADR 0057 §4) and the host-control
backplane region the audio side reads from.

## Acceptance criteria

- [ ] Single `HostControl` module in `patches-modules` with two
      output ports, alias-indexed:
      `audio_out[name]` (Mono+Audio) for knob / slider / toggle and
      `trigger_out[name]` (Mono+Trigger) for trigger. No inputs.
      Mirrors the Tap-module split (ADR 0059 §4) — one synth
      instance, kind-suffixed ports.
- [ ] Per-channel params: `slot_offset: usize`, `kind: enum
      { knob, slider, toggle, trigger }`. Kind drives which output
      port the channel publishes on; slot is global / alphabetical
      (ADR 0057 §3).
- [ ] Per-tick action per channel: copy `backplane[slot_offset]` to
      the output port matching the channel's kind. No allocation,
      no branching beyond the kind dispatch.
- [ ] Backplane region for host control distinct from the tap
      region (ADR 0053 plumbing already accommodates this).
- [ ] Audio side reads with `Acquire`; control side writes with
      `Release`. Plain `f32` stores; tearing acceptable per ADR 0057
      §4.
- [ ] Sub-block accuracy explicitly out of scope: one value per
      block per control. Document in module doc comment.
- [ ] Module documented in standard form (CLAUDE.md "Module
      documentation standard").
- [ ] Tests: zero-input determinism, slot read correctness, channel
      shape change.
- [ ] `just inner -p patches-modules -p patches-engine` passes.

## Notes

- Audio side does not see the manifest. It operates from
  `(slot_offset, channel_count)` baked into the instance.
- ADR 0050 ramp primitive available downstream where per-sample
  smoothing of the block-rate value is needed; not this module's
  job.
