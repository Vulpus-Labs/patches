---
id: "E030"
title: FDN Reverb module
status: closed
priority: medium
created: 2026-03-22
tickets:
  - "0175"
  - "0176"
  - "0177"
---

## Summary

Adds a stereo `FdnReverb` module to `patches-modules`: an 8-line Feedback Delay
Network reverb with Hadamard mixing matrix, per-line high-shelf absorption, and
Thiran all-pass interpolation on each delay line for LFO modulation without
pitch artifacts.

The module exposes three user-facing parameters — `size`, `brightness`, and
`character` — that map onto the underlying physical parameters (delay line
lengths, RT60 at low and high frequencies, absorption crossover) according to
per-character curves designed to match the acoustic signature of each space type.

## Parameters

| Parameter    | Type  | Range  | Default  | Meaning |
|--------------|-------|--------|----------|---------|
| `size`       | Float | [0, 1] | `0.5`    | Room scale; drives delay line lengths and RT60 |
| `brightness` | Float | [0, 1] | `0.5`    | Tonal character within the chosen space type |
| `character`  | Enum  | —      | `"hall"` | Space archetype; determines how size and brightness are mapped |

`size` and `brightness` each have a corresponding `_cv` MonoInput (additive,
clamped to [0, 1]).

## Character archetypes

| Value        | Space type                              |
|--------------|-----------------------------------------|
| `"plate"`    | Dense, uniform metal-plate decay        |
| `"room"`     | Small-medium natural room               |
| `"chamber"`  | Small reflective stone/tile space       |
| `"hall"`     | Concert hall                            |
| `"cathedral"`| Very large reverberant stone interior   |

Each archetype specifies:
- Delay line scale range (`min_scale..max_scale` mapped from size via exponential curve)
- LFO rate and depth per delay line (to break up metallic resonances)
- Pre-delay maximum (scaled by size)
- Brightness curve: `(crossover_hz, lf_hf_ratio)` interpolated over brightness [0, 1]

## Signal flow

```
in_l ──(+in_r copy if mono-in stereo-out)──► [pre-delay]
                                                   │
                              ┌────────────────────┘
                              ▼
               delay_0 ► [absorption_0] ──►─╮
               delay_1 ► [absorption_1] ──►─┤
               ...                          │
               delay_7 ► [absorption_7] ──►─┤
                              ▲              │
                              └── [Hadamard] ◄╯
                                      │
                               output gains L/R
                                      │
                               out_l    out_r
```

Absorption is a high-shelf filter (MonoBiquad) per delay line, with DC gain
and HF gain derived from the delay line length, size (→ RT60), and brightness
(→ crossover + LF/HF ratio).  The Hadamard FWHT (3 butterfly passes for N=8,
normalised by 1/√8) provides dense, lossless mixing.  Delay lengths are read
via ThiranInterp at a continuously modulated offset (sine LFO per line).

## Stereo handling

Uses `set_connectivity` to detect which of `in_r`/`out_r` are wired:
- `out_r` disconnected → mono mode, only `out_l` computed
- `in_r` disconnected but `out_r` connected → inject `in_l` into both L and R delay subsets
- Both connected → full stereo injection and output

L and R output gain vectors are orthogonal sign patterns over the 8 lines,
giving natural channel decorrelation from the Hadamard structure at no extra cost.

## Tickets

- [T-0175](../tickets/closed/0175-delay-buffer.md) — `DelayBuffer`, `ThiranInterp`, poly variants *(closed)*
- [T-0176](../tickets/closed/0176-fdn-reverb-module.md) — `FdnReverb` module implementation
- [T-0177](../tickets/open/0177-fdn-reverb-registry-and-tests.md) — Register, integration tests, example patch
