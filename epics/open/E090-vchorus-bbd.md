---
id: "E090"
title: VChorus — Juno-60-style chorus with reusable BBD core
created: 2026-04-18
tickets: ["0552", "0553"]
---

## Goal

Ship `VChorus` (vintage chorus), a Juno-60-style stereo chorus module,
built on a reusable BBD primitive in `patches-dsp` that will later
underpin a vintage BBD delay module (CE-2 / Small Clone territory).

A separate modern digital chorus module will be planned later and is
out of scope for this epic.

## Background — Juno-60 chorus

Hardware (sources: pendragon-andyh/Juno60 repo,
florian-anwander.de/roland_string_choruses, Juno-60 service notes):

- 2× **MN3009** BBDs (256 stages), **MN3101** clock. BBD rate ~70 kHz.
- **No compander** (unlike Juno-106 / CE-2).
- **Single triangle LFO**, right channel reads inverted LFO — not two
  phase-offset LFOs. Strict linear triangle (op-amp integrator +
  Schmitt trigger). Mono-compatibility depends on the inversion.
- Pre-BBD 12 dB/oct LPF, post-BBD ~3rd-order reconstruction LPF
  (~10 kHz).
- Fixed dry/wet via summing resistors; no user mix control.

Mode table (pendragon-andyh measurements):

| Mode | LFO Hz | Delay min | Delay max | Swing   |
| ---- | ------ | --------- | --------- | ------- |
| I    | 0.513  | 1.66 ms   | 5.35 ms   | 3.69 ms |
| II   | 0.863  | 1.66 ms   | 5.35 ms   | 3.69 ms |
| I+II | 9.75   | 3.30 ms   | 3.70 ms   | 0.40 ms |

Character — what gives a Juno its sound (filters aside):

- **Charge-transfer inefficiency (CTI)** — per-stage charge loss
  produces a delay-dependent HF rolloff that darkens as delay grows.
  Captured by Holters & Parker, DAFx-18
  (<https://www.dafx.de/paper-archive/2018/papers/DAFx2018_paper_25.pdf>),
  implemented in ChowDSP
  (<https://github.com/Chowdhury-DSP/chowdsp_utils> BBD module).
- **Hiss** — MN3009 uncompanded gives SNR ~55–65 dB, broadband with
  mild HF tilt. Dominant noise source; clock feedthrough is mostly
  filtered. Model as white noise into wet path, LPF'd, scaled by wet
  level.
- Clock aliasing is inaudible at Juno delays (70 kHz clock, 1–7 ms
  delays); the Holters-Parker model captures it anyway, "for free".

## Why Holters-Parker

~400 LOC C++ reference → ~500 LOC Rust, real-time cheap
(~20–40 flops/sample stereo). Two banks of 4 parallel complex
one-pole filters (input anti-image, output reconstruction) bracketing
a bucket ring buffer; inner loop advances BBD clock ticks
(~1.5 per host sample at Juno rates). Precompute `sincos` on
delay-rate change; inputs/outputs are 4-wide complex pole sets shipped
as per-device constants.

For Juno alone, a delay-dependent one-pole LPF would get ~90%. But
this BBD core will be reused for CE-2/Small-Clone-class modules where
clock drops to 5–20 kHz and images fold into audio band — at which
point the full model matters clearly. Write once, use everywhere.

No existing Rust port of Holters-Parker is known; this crate becomes
the reference implementation.

## Tickets

| ID   | Title                                               | Priority | Depends on |
| ---- | --------------------------------------------------- | -------- | ---------- |
| 0552 | BBD core (Holters-Parker) in patches-dsp            | medium   | —          |
| 0553 | VChorus module in patches-modules                   | medium   | 0552       |

## Out of scope

- Modern digital chorus module (separate epic later).
- Vintage BBD delay module (future consumer of the BBD core; separate
  ticket once VChorus lands and BBD API is proven).
- Per-stage BBD saturation nonlinearity (v2; audible only hot).

## References

- <https://github.com/pendragon-andyh/Juno60/blob/master/Chorus/README.md>
- <https://www.florian-anwander.de/roland_string_choruses/>
- <https://www.dafx.de/paper-archive/2018/papers/DAFx2018_paper_25.pdf>
- <https://github.com/Chowdhury-DSP/chowdsp_utils> — `chowdsp_BBDFilterBank.h`,
  `chowdsp_BBDDelayLine.h`
- Juno-60 service notes, archive.org
