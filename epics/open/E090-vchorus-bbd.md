---
id: "E090"
title: VChorus — vintage BBD chorus module with reusable BBD core
created: 2026-04-18
tickets: ["0552", "0553", "0554", "0555"]
---

## Goal

Ship `VChorus` (vintage chorus), a stereo BBD chorus module with two
voicings (`bright` and `dark`) inspired by two well-known early-80s
Roland BBD choruses, built on a reusable BBD primitive in a new
`patches-vintage` crate that will later underpin a vintage BBD delay
module (CE-2 / Small Clone territory).

### Crate placement

Everything in this epic lives in a new **`patches-vintage`** workspace
crate: the BBD core, the compander primitive, and the VChorus module.
`patches-modules` adds a path dependency on `patches-vintage` and
calls its registration hook from `default_registry()` so VChorus is
available through the default module set with no DSL-surface change.

These are add-on effects, not core modular primitives. A later epic
will convert `patches-vintage` into a dynamically loadable plugin
bundle via the FFI mechanism already landed in E088; at that point
the static dependency in `patches-modules` is removed. Keeping the
crate separate from day one makes that future move mechanical.

A separate modern digital chorus module will be planned later and is
out of scope for this epic.

## Trademark / naming policy

"Juno" is a Roland trademark. This epic and related tickets cite the
Juno-60 and Juno-106 as hardware references under nominative fair use
— the discussion of what the effects do is accurate historical
description. User-facing names **must not** use the Roland names:

- Module name: `VChorus` (generic "vintage chorus").
- Variant enum values: `bright` and `dark` (descriptive of voicing),
  not `juno60` / `juno106`.
- Mode values: `one`, `two`, `both`, `off` (Roman numerals on the
  original hardware; generic ordinals here).

Implementation comments and design documents may reference the
Juno-60/Juno-106 for precision. Marketing copy and module
descriptions shown to users must not.

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

## `dark` variant (Juno-106 reference)

VChorus ships both `bright` (Juno-60 reference) and `dark` (Juno-106
reference) voicings. Contrary to common belief, the Juno-106 chorus
**does not** use a compander (verified against service notes and
florian-anwander.de). The "quieter 106" comes from gain-staging and a
darker reconstruction filter, not companding.

Actual Juno-106 differences from the Juno-60 (informing the `dark`
voicing):

- Same 2× MN3009 + MN3101 BBDs.
- **Two modes only** (I, II). No `I+II` fast mode. Inversion is not
  user-defeatable (always stereo).
- Darker post-BBD reconstruction filter (~7 kHz corner vs 60's
  ~9 kHz).
- Closer to 50/50 dry/wet summing (60 runs wet hotter).
- Chorus is always in-path — "off" means "no modulation", signal
  still traverses the BBD (slight colouration even at rest).
- ~6–8 dB quieter noise floor from the above, no companding.

The user asked for a "106 mode with companding"; since companding is
not historically accurate for the 106, the 106 preset is faithful and
the compander lands in ticket 0555 as a reusable primitive for
future Dimension-D / CE-2 / Small-Clone modules (which do compand).

## Tickets

| ID   | Title                                                | Priority | Depends on |
| ---- | ---------------------------------------------------- | -------- | ---------- |
| 0552 | Scaffold `patches-vintage` crate                     | medium   | —          |
| 0553 | BBD core (Holters-Parker) in patches-vintage         | medium   | 0552       |
| 0554 | VChorus module + registry hook into patches-modules  | medium   | 0553       |
| 0555 | Compander primitive (NE570-style) in patches-vintage | low      | 0552       |

## Out of scope

- Modern digital chorus module (separate epic later).
- Vintage BBD delay module and Dimension-D-style module (future
  consumers of BBD core + compander; separate tickets once VChorus
  lands and the APIs are proven).
- Per-stage BBD saturation nonlinearity (v2; audible only hot).

## References

- <https://github.com/pendragon-andyh/Juno60/blob/master/Chorus/README.md>
- <https://www.florian-anwander.de/roland_string_choruses/>
- <https://www.dafx.de/paper-archive/2018/papers/DAFx2018_paper_25.pdf>
- <https://github.com/Chowdhury-DSP/chowdsp_utils> — `chowdsp_BBDFilterBank.h`,
  `chowdsp_BBDDelayLine.h`
- Juno-60 service notes, archive.org
