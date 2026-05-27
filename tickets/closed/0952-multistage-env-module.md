---
id: "0952"
title: "Env: multi-stage envelope module, mono (patches-modules)"
priority: medium
created: 2026-05-22
epic: E155
---

## Summary

Wrap the 0951 `EnvCore` in a mono `patches-modules` module named `Env`, modelled
on the existing `Adsr` module (`patches-modules/src/modulators/adsr.rs`). Adds
the two capabilities ADSR lacks: **key-follow time-scaling** (stage times shorten
with pitch, as real resonators decay faster up the keyboard) and **velocity
scaling**, plus the same built-in VCA pass-through `Adsr` has so it can directly
shape a signal.

Key-follow is a hard requirement, not polish: it is what later lets one envelope
serve a whole keyboard zone and (once the deferred sampler lands) keeps a
transient-shaping envelope aligned with a pitched sample whose length changes
with pitch.

## Acceptance criteria

- [x] Module `Env` in `patches-modules/src/modulators/env.rs`, registered in
      `default_registry()` and listed in the registry test.
- [x] **Stage count is configurable** via the descriptor. **Locked shape
      (confirmed with requester):** the `CHANNELS` count axis is repurposed as
      the stage count (`Env(5)` = 5 stages) — it is the only axis wired through
      `ModuleShape` today. Per-stage `(time, level, curve)` are
      `per_axis_realtime_params`; the sustain-stage index is a structural int
      param. One envelope per instance (mono); multiple envelopes = multiple
      instances. Rejected the fixed-`MAX_STAGES`+struct-count shape (always-8
      param slots / messy indexing).
- [x] Inputs: `trigger`, `gate`, `voct`, `velocity`, `vca_in`;
      `vca_out = vca_in * env`; `out` raw envelope.
- [x] **Key-follow**: `keyfollow` (`0..1`) + `ref_key`; module computes
      `time_scale = 2^(-keyfollow * (voct - ref_key))` per tick. `keyfollow =
      1.0` halves stage times one octave up.
- [x] **Velocity scaling**: `velocity` → level multiplier
      `1 - vel_depth*(1-velocity)`, pushed via `EnvCore::set_level_scale` and
      latched at trigger inside the core. Level-only, consistent with the 0951
      decision (key-follow stays the separate per-tick `time_scale`).
      Unconnected `velocity` reads as 1.0 (no attenuation).
- [x] Connectivity guards (`is_connected()` on `voct`/`velocity`/`out`/`vca_out`).
- [x] Module doc-comment in the standard form; manual page
      (`docs/src/modules/envelopes.md`) updated to match.
- [x] Tests: key-follow halves time one octave up; velocity scales level;
      unconnected velocity = full; VCA pass-through equals `in * env`;
      sustain-then-release-tail; idle output zero.

## Resolution

- Explicit `sustain_stage` index (structural int, range 0–7), not penultimate:
  stages after it form a multi-stage release tail (matches the 0951 core).
- `EnvCore::tick(triggered, gate_high, time_scale)` driven once per tick; stage
  list rebuilt on the stack (no alloc) in `update_validated_parameters` and
  handed to `set_stages`, with `set_sustain_stage` re-applied after (set_stages
  re-clamps the index).

## Notes

- Authoring pattern (template/prepare/update_validated_parameters/set_ports/
  process, `module_params!`) per `patches-modules/src/osc/oscillator.rs` and the
  `Adsr` module.
- Routing the `out` to an oscillator `voct`/`phase_mod` or a filter cutoff gives
  the D50 attack pitch-blip / filter sweep — document as a patch idiom, not a new
  port.
- Depends on 0951. Validation: `just inner -p patches-modules`.
