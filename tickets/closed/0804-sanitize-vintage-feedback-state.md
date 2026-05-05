---
id: "0804"
title: Sanitize vintage feedback state (defense in depth)
priority: medium
created: 2026-05-04
---

## Summary

Per-site denormal flush in patches-vintage feedback paths. Backstop
for FTZ/DAZ (ticket 0802): covers offline render, tests, and any
future host that resets MXCSR mid-callback.

## Acceptance criteria

- [ ] `vflanger/core.rs` `fb_state` write (line ~162): flush_denormal.
- [ ] `vflanger_stereo/core.rs` `fb_state` writes (both channels).
- [ ] `vbbd.rs` per-tap `fb_state`, `fb_hp_y_prev`, `fb_lp_y_prev`
      writes.
- [ ] `vstereobbd.rs` cross-feedback paths (pingpong).
- [ ] `vreverb.rs` damping cascade `damp_z1`, `damp_z2` after each
      update.
- [ ] `bbd_filter_proto.rs` `x_re` / `x_im` complex pole state after
      transient ring.
- [ ] Use the existing `flush_denormal` helper from patches-dsp;
      don't reinvent.
- [ ] `just inner -p patches-vintage` passes.

## Notes

- Threshold and helper already exist (used by `dc_blocker`,
  `envelope_follower`).
- Don't change algorithm output for normal-range signals; flush only
  fires below ~1e-30.
