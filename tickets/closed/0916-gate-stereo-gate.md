---
id: "0916"
title: Gate + StereoGate
priority: medium
created: 2026-05-18
epic: E151
---

## Summary

Threshold gate with hysteresis, attack / hold / release ballistics,
and a sidechain port. Stereo variant uses the linked detector
established in ticket 0915 / ADR 0076. One gate state drives both
channels.

No `mix` parameter — gating is binary in intent; a dry/wet blend
muddies the semantics. A patch author who wants ducking-with-blend
uses `Compressor` with high ratio and `mix < 1`.

## Parameters

| Name | Type | Range | Default | Description |
|------|------|-------|---------|-------------|
| `threshold` | float | −80..0 dB | `-40` | Open threshold |
| `hysteresis` | float | 0..24 dB | `3` | Re-arm band below threshold |
| `attack` | float | 0.01..1000 ms | `1` | Open ramp |
| `hold` | float | 0..5000 ms | `10` | Minimum open time once triggered |
| `release` | float | 1..5000 ms | `100` | Close ramp |

## Acceptance criteria

- [ ] `Gate` and `StereoGate` registered; descriptors per ADR 0076.
- [ ] Kernel test: state machine open/close around threshold with
      hysteresis — signal at `threshold - hysteresis / 2` after an
      open event keeps the gate open; signal below
      `threshold - hysteresis` closes it.
- [ ] Kernel test: hold time prevents close even if signal drops
      below `threshold - hysteresis` within the hold window.
- [ ] Surface test: sidechain self-key fallback.
- [ ] Stereo surface test: asymmetric L/R signal produces identical
      gate state on L and R (linked detector invariant).
- [ ] Manual page added under `docs/src/modules/`.
- [ ] `just commit -p patches-modules` green.

## Notes

Hysteresis is the same trap as in 0917: it controls *eligibility*, not
event timing. The gate opens when `armed && signal > threshold`; it
does not open at `threshold - hysteresis`. Document this at the
state-machine site to head off future "fixes".
