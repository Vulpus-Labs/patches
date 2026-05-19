---
id: "0919"
title: Stereo utility — Pan, Balance, StereoWidth, MidSide, MonoBass
priority: medium
created: 2026-05-18
epic: E151
---

## Summary

Five stereo image utilities per ADR 0076. All ship in the new
`stereo/` group directory (ticket 0922) alongside the existing
`stereo_split` / `stereo_sum`.

| Module | In | Out | Notes |
|--------|----|----|-------|
| `Pan` | mono + `pan` CV | stereo | Equal-power (sin/cos, −3 dB centre); `pan: -1..1` |
| `Balance` | stereo + `balance` CV | stereo | Linear −6 dB; matches mixer pan law |
| `StereoWidth` | stereo + `width` CV | stereo | Internal M-S; scale S by `width`, leave M; `width: 0..2` |
| `MidSide` | `stereo_in: stereo`, `ms_out: stereo`, `ms_in: stereo`, `stereo_out: stereo` | bidirectional | Pure relabel; either path usable standalone |
| `MonoBass` | stereo + `cutoff` CV | stereo | LR4 crossover; below cutoff → `(L+R)/2` both; default 120 Hz |

`MidSide` cables are all `Stereo`-kind; the M-S form is a stereo
cable carrying `(M, S)` rather than `(L, R)`. No descriptor metadata
distinguishes M-S stereo from L-R stereo (same constraint as any
DAW's mid/side workflow).

## Acceptance criteria

- [ ] Five modules registered with descriptors matching ADR 0076.
- [ ] `Pan`: equal-power law verified — `pan = 0` outputs `(in / √2,
      in / √2)` (−3 dB on each channel; full power preserved).
- [ ] `Balance`: linear −6 dB at extremes matches the mixer's pan
      law (cross-reference the mixer law in test).
- [ ] `StereoWidth`: `width = 0` → mono sum on both channels;
      `width = 1` → identity (bit-exact `(L, R)` round-trip through
      M-S and back).
- [ ] `MidSide`: encode/decode round-trip is within `ε = 1e-7` of
      input.
- [ ] `MidSide` partial-wiring surface test: encode-only and
      decode-only patches each produce expected output with the
      unused path unconnected.
- [ ] `MonoBass`: Linkwitz-Riley 4th order — crossover gain at
      cutoff is `-6` dB on each side (LR4 sum-flat property).
- [ ] Manual pages added under `docs/src/modules/`.
- [ ] `just commit -p patches-modules` green.

## Notes

LR4 = two cascaded Butterworth 2nd-order biquads. The biquad kernel
in `patches-dsp` already supports the coefficients; no new DSP code,
just two filter instances with the same cutoff.

`StereoWidth` round-trip identity at `width = 1` is the easiest
regression-trap: any drift means the M-S encode/decode constants
have lost their symmetry.
