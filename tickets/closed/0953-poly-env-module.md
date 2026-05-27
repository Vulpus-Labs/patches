---
id: "0953"
title: "PolyEnv: 16-voice multi-stage envelope (patches-modules)"
priority: medium
created: 2026-05-22
epic: E155
---

## Summary

Add a 16-voice variant of the `Env` module (0952), following the existing
`PolyAdsr` pattern (`patches-modules/src/modulators/poly_adsr.rs`): fixed
`[f32; 16]` poly cables, per-voice state. Completes the envelope for use in the
fundamentally 16-voice system (every other modulator has a poly form).

## Acceptance criteria

- [x] Module `PolyEnv` in `patches-modules/src/modulators/poly_env.rs`,
      registered in `default_registry()` + registry test, mirroring `PolyAdsr`.
- [x] Per-voice poly `trigger`, `gate`, `voct`, `velocity`; per-voice key-follow
      and velocity scaling; `vca_out[v] = vca_in[v] * env[v]`.
- [x] `[EnvCore; 16]` (`std::array::from_fn`), advanced per voice; stage list
      built on the stack in `update_validated_parameters` — no RT allocation.
- [x] Stage-count descriptor matches 0952: `CHANNELS` axis = stage count,
      per-stage `time/level/curve`, structural `sustain_stage`. Ports are global
      poly (the stages axis sizes params only, like mono `Env`); the 16 voices
      ride poly cables, not an axis.
- [x] Module doc-comment in the standard form; manual page
      (`docs/src/modules/envelopes.md`) updated with a `PolyEnv` section.
- [x] Tests (7): per-voice key-follow, per-voice velocity, unconnected velocity
      = full, per-voice VCA, voice independence, sustain-then-release, idle zero.

## Notes

- Voice allocation already exists (`PolyMidiToCv`, LIFO steal); `PolyEnv` just
  consumes per-voice poly cables.
- Model directly on `PolyAdsr` for per-voice trigger/gate routing and the VCA
  pass-through.
- Depends on 0952. Validation: `just commit -p patches-modules` before closing.
