---
id: "0918"
title: AudioToGate (mono / stereo / poly)
priority: medium
created: 2026-05-18
epic: E151
---

## Summary

Sustained gate output driven by an audio input. Reuses the hysteresis
state machine from ticket 0917 / ADR 0076: gate is high while
`armed && signal > threshold`, falls when
`signal < threshold - hysteresis`. No sub-sample reporting — gates
are sample-accurate per ADR 0030.

Stereo variant uses the linked detector (`max(|L|, |R|)`) and emits
one mono gate output. Poly variant runs an independent state machine
per channel and emits a poly gate output.

## Parameters

| Name | Type | Range | Default | Description |
|------|------|-------|---------|-------------|
| `threshold` | float | −60..0 dB or linear 0..1 | `-12` dB | Open threshold |
| `hysteresis` | float | 0..24 dB | `3` | Close band below threshold |

## Acceptance criteria

- [ ] `AudioToGate`, `StereoAudioToGate`, `PolyAudioToGate`
      registered; descriptors per ADR 0076.
- [ ] Kernel test: gate opens when signal crosses `threshold` rising,
      stays open while signal stays above `threshold - hysteresis`,
      closes when it drops below.
- [ ] Kernel test: small oscillations within the hysteresis band
      do not toggle the gate.
- [ ] Stereo surface test: linked detector — asymmetric L/R signal
      produces a single deterministic gate, not two independent
      ones.
- [ ] Poly surface test: per-channel state — one channel above
      threshold while another is below produces independent gate
      states.
- [ ] Manual page added under `docs/src/modules/`.
- [ ] `just commit -p patches-modules` green.

## Notes

The kernel from 0917 can be reused — `AudioToGate` is the same state
machine with a different output projection. Either share via a kernel
function or via a `Mode::{Trigger, Gate}` enum on a unified detector;
pick whichever keeps the kernel tests cleanest.
