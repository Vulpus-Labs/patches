---
id: "0917"
title: AudioToTrigger (mono / stereo / poly)
priority: medium
created: 2026-05-18
epic: E151
---

## Summary

Edge detector that converts an audio signal into ADR 0047 sub-sample
sync events. Fire condition is `armed && prev <= threshold &&
curr > threshold` for the rising direction (falling and bidirectional
follow symmetrically). Sub-sample fraction
`frac = (threshold - prev) / (curr - prev)` is the ADR 0047 event
fraction. The fire threshold is `threshold`, never
`threshold - hysteresis`.

Hysteresis is a two-state machine (`armed` ↔ `disarmed`). Re-arm in
the rising direction requires `signal < threshold - hysteresis`.
**Hysteresis controls eligibility, never event location.** Document
this at the interp line and in the module doc comment so a future
reader does not "fix" the asymmetry.

Stereo variant detects on `max(|L|, |R|)`. Poly variant runs an
independent detector per channel.

## Parameters

| Name | Type | Range | Default | Description |
|------|------|-------|---------|-------------|
| `threshold` | float | −60..0 dB or linear 0..1 | `-12` dB | Fire threshold |
| `hysteresis` | float | 0..24 dB | `3` | Re-arm band |
| `direction` | enum | `rising` / `falling` / `both` | `rising` | Edge polarity |
| `cooldown` | float | 0..1000 ms | `0` | Debounce after fire |

## Acceptance criteria

- [ ] `AudioToTrigger`, `StereoAudioToTrigger`, `PolyAudioToTrigger`
      registered; descriptors per ADR 0076.
- [ ] Kernel test: linear interpolant — synthetic signal crossing
      threshold mid-sample produces an event with the analytically
      correct fraction within `1e-6`.
- [ ] Kernel test: re-arm with hysteresis — a series of small
      oscillations around `threshold` between
      `threshold - hysteresis / 2` and `threshold + ε` produces
      exactly one event (not many).
- [ ] Kernel test: cooldown suppresses re-fires within the window
      even if the signal re-arms.
- [ ] Kernel test: falling direction fires at `threshold` not at
      `threshold + hysteresis` (the symmetric trap).
- [ ] Surface test: `direction` parameter selects the right kernel
      branch.
- [ ] Stereo / poly surface test: descriptor shape and per-channel
      independence for the poly variant.
- [ ] Manual page added under `docs/src/modules/`.
- [ ] `just commit -p patches-modules` green.

## Notes

The interp formula divides by `curr - prev`. Guard against `curr ==
prev` (no crossing inside the sample); the fire condition already
implies `prev <= threshold < curr` so the divisor is strictly
positive in the rising case, but a defensive `max(eps, ...)` is
cheaper than reasoning about every branch.
