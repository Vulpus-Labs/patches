---
id: E155
title: Multi-stage envelope (Env)
status: done
created: 2026-05-22
closed: 2026-05-22
---

## Goal

Add a multi-breakpoint envelope module, `Env`, to the repertoire: an
arbitrary number of `(time, level, curve)` stages with a designated sustain
stage, plus **key-follow time-scaling** (stage times shorten with pitch) and
velocity scaling. This is the D50 lesson made real — the contours ADSR can't
express (attack spike → dip → secondary swell → sustain) and the
pitch-dependent timing that lets one envelope serve a whole keyboard zone.

Originally the head of a larger pitched-transient voice epic (D50-style sample
attack + synth sustain). The sampler half is **deferred to `patches-bundles`**,
which already has an IO module for loading IRs/samples; the envelope is the part
that's generally useful now and unblocks that work later.

## Scope

**In:**

- `EnvCore` — pure, alloc-free multi-stage envelope state machine
  (`patches-dsp`).
- `Env` — mono module (`patches-modules`) with key-follow time-scaling,
  velocity scaling, and VCA pass-through.
- `PolyEnv` — 16-voice variant.

**Out (deferred):**

- Mipmapped sample player + sample loading — moves to `patches-bundles` (IO
  module there).
- The pitched-transient voice patch (sample attack VCA'd against synth sustain).
  Revisit as a follow-up once the sampler lands in `patches-bundles`; the splice
  is just two partials each VCA'd by an `Env`, summed — no crossfader needed.
- Full D50 emulation (partial structure table, onboard chorus/reverb, joystick).

## Tickets

- [x] [0951 — EnvCore: multi-stage envelope core (patches-dsp)](../tickets/closed/0951-multistage-env-core.md)
- [x] [0952 — Env: multi-stage envelope module, mono (patches-modules)](../tickets/closed/0952-multistage-env-module.md)
- [x] [0953 — PolyEnv: 16-voice variant (patches-modules)](../tickets/closed/0953-poly-env-module.md)

## Dependency order

```text
0951 (EnvCore) ──> 0952 (Env, mono) ──> 0953 (PolyEnv)
```

## Acceptance

- `Env` plays an arbitrary-stage contour with a held sustain stage and a release
  tail.
- Stage times track pitch via key-follow (`keyfollow = 1.0` halves times one
  octave up).
- Velocity scales attack rate / peak level.
- VCA pass-through equals `in * env`, matching the `Adsr` module's built-in VCA.
- `PolyEnv` does the above per-voice across 16 voices.
- `just commit` green for touched crates; `cargo clippy` clean.

## Open questions

1. **"Channel-per-stage" descriptor shape.** Likely reading: stage count as a
   `CountAxis` with per-stage `(time, level, curve)` as `per_axis_realtime_params`,
   plus a structural sustain-stage index. Confirm interpretation in 0952.
2. **Key-follow ↔ resample coupling (future).** When the deferred sampler pitches
   a sample up by ratio `r`, its duration shrinks by `1/r`; full key-follow
   (`1.0`) makes envelope times scale the same way, keeping a transient envelope
   aligned with the sample. Not needed now, but the reason key-follow is a hard
   requirement rather than polish.
