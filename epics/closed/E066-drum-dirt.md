---
id: "E066"
title: Distortion, bitcrushing, and transient shaping for drums
created: 2026-04-12
tickets: ["0353", "0354", "0355"]
---

## Summary

Three new effect modules aimed at dirtying up drum sounds:

1. A **Bitcrusher** module for sample-rate reduction and bit-depth
   reduction — classic lo-fi degradation with CV-modulatable rate and
   depth. DSP kernel lives in `patches-dsp` for independent testing.

2. A **Drive** module offering multiple distortion algorithms (tanh
   saturation, wavefolding via `fast_sine`, hard clipping, and a
   crushed-digital mode) with pre/post DC blocking, asymmetric bias,
   post-distortion tone control, and dry/wet mix. Designed for
   musically useful saturation across kicks, snares, and full bus.

3. A **TransientShaper** module using dual envelope followers (fast
   and slow) to independently boost or cut attack transients and
   sustained tails. Essential for adding punch or taming dynamics on
   drum hits. The envelope follower DSP goes in `patches-dsp` as a
   reusable primitive.

## Tickets

| Ticket | Title                                  |
| ------ | -------------------------------------- |
| 0353   | Bitcrusher module and DSP kernel       |
| 0354   | Drive module (multi-mode distortion)   |
| 0355   | TransientShaper module                 |

The tickets are independent and can be worked in parallel. All three
share the pattern of a mono-in/mono-out effect with CV modulation and
dry/wet control.
