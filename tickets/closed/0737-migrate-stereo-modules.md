---
id: "0737"
title: Migrate stereo modules to single stereo ports
priority: high
created: 2026-04-27
---

## Summary

Replace paired `*_left`/`*_right` mono ports with a single stereo port
on modules where both halves carry one logical stereo signal:
`stereo_delay`, `stereo_limiter`, mixer stereo + stereo_poly variants,
`convolution_reverb` (stereo path), `fdn_reverb`, `audio_in`,
`audio_out`. Update each module's processing loop to read/write through
`StereoInput`/`StereoOutput`.

## Acceptance criteria

- [ ] Each listed module's descriptor uses `.stereo_in()` /
      `.stereo_out()` for symmetric stereo ports.
- [ ] Module doc-comment tables updated (`stereo` kind in the Inputs /
      Outputs columns).
- [ ] Per-module unit tests updated and passing.
- [ ] `cargo clippy` clean.
- [ ] Compound stereo names (e.g. mixer `send_a_left`/`send_a_right`)
      collapse to `send_a`, etc.

## Notes

ADR 0059 §3. Modules with semantically distinct halves (mid/side, if
any are added later) keep paired mono ports. None of the current
modules qualify for that exception. Update CLAUDE.md port-naming
section under 0743.
