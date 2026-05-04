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

- [ ] `HostControl` module in `patches-modules` with descriptor
      `out[i]`, `i ∈ 0..channels`, `MonoLayout::Audio`. No inputs.
- [ ] Per-channel params: `name: String`, `slot_offset: usize`.
- [ ] Per-tick action: `out[i] = backplane[slot_offset + i]`. No
      allocation, no branching on kind.
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
