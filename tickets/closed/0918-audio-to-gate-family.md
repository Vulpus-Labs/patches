---
id: "0918"
title: AudioToGate (mono / poly)
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

- [x] `AudioToGate` and `PolyAudioToGate` registered; descriptors per
      ADR 0076. (`StereoAudioToGate` dropped — see Resolution.)
- [x] Kernel test: gate opens when signal crosses `threshold` rising,
      stays open while signal stays above `threshold - hysteresis`,
      closes when it drops below.
- [x] Kernel test: small oscillations within the hysteresis band
      do not toggle the gate.
- [x] Poly surface test: per-channel state — one channel above
      threshold while another is below produces independent gate
      states.
- [x] Manual page added under `docs/src/modules/`.
- [x] `just commit -p patches-modules` green.

## Notes

The kernel from 0917 can be reused — `AudioToGate` is the same state
machine with a different output projection. Either share via a kernel
function or via a `Mode::{Trigger, Gate}` enum on a unified detector;
pick whichever keeps the kernel tests cleanest.

## Resolution

Implemented `AudioToGate` and `PolyAudioToGate` using a dedicated
`GateSchmitt` kernel in `detectors/common/gate_schmitt.rs` — separate
from `EdgeDetector` because the gate machine has no cooldown, no
direction enum, and a one-bit sustained-state output. Keeping the two
kernels in their own files makes the tests focused (schmitt invariants
vs sub-sample interp + arm machine).

`StereoAudioToGate` (originally listed in ADR 0076) was dropped. The
same rationale that ruled out `StereoAudioToTrigger` applies: the mono
gate compares signed samples (`signal > threshold`), and there is no
consistent collapse of two signed streams to one — `max(|L|, |R|)`
would silently change the gate's semantics from a signed schmitt at
oscillator rate to an envelope-above-threshold detector, which is a
different operation from the mono variant at the same threshold. A
patch wanting envelope-above-threshold from a stereo bus composes a
stereo peak / RMS summariser module (not yet in tree — follow-up if a
use case appears) with `AudioToGate`. ADR 0076 and
`docs/src/modules/detectors.md` updated to record this.
