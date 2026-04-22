---
id: "0602"
title: VVcf / VPolyVcf — 4-pole ZDF ladder LPF with Juno60/106 variants
priority: medium
created: 2026-04-20
epic: E102
---

## Summary

Add `VVcf` (mono) and `VPolyVcf` (poly) to `patches-vintage`: a
4-pole zero-delay-feedback ladder low-pass with per-stage tanh
saturation and self-oscillation at full resonance. A `variant`
parameter selects between Juno-60 (IR3109-ish, crisper) and Juno-106
(80017A-ish, softer, slight HF loss, resonance compresses bass)
coefficient sets. Mono and poly variants share the ladder kernel;
wrappers differ only in port kind and per-voice state fan-out.

## Design

### DSP kernel

- New `patches-dsp::ladder` module (sibling of `biquad`, `svf`).
- ZDF topology (Zavalishin). One-pole per stage, four stages, global
  feedback with tanh on the feedback path — or per-stage tanh, choose
  whichever gives cleaner self-oscillation behaviour.
- Self-oscillating at `resonance = 1.0`.
- Variant coefficients baked in as constants; variant is a per-sample
  branch (cheap) or a small dispatch at cutoff-update time.

### Variants

- `Juno60` — crisper, more aggressive resonance peak. Output tap at
  the 4th stage.
- `Juno106` — softer resonance, slight HF loss, bass compresses with
  resonance (typical 80017A behaviour). A modest shelf or blended tap
  reproduces this; document the chosen method in a comment.

Use the `params_enum!` macro (see ADR 0045 Spike 0) for the variant
param so module code reads `Variant::Juno106` not a raw `u32`.

### Parameters

| Name        | Type   | Range           | Default   | Description                      |
| ----------- | ------ | --------------- | --------- | -------------------------------- |
| `variant`   | enum   | Juno60, Juno106 | `Juno106` | Filter voicing                   |
| `cutoff`    | float  | Hz              | `1000.0`  | Base cutoff frequency            |
| `resonance` | float  | 0..1            | `0.0`     | Feedback amount; self-osc near 1 |
| `drive`     | float  | 0..4            | `1.0`     | Input gain into tanh stages      |

### Inputs

| Port        | Kind   | Description                   |
| ----------- | ------ | ----------------------------- |
| `in`        | poly   | Audio input                   |
| `cutoff_cv` | poly   | Cutoff modulation (v/oct sum) |

### Outputs

| Port   | Kind   | Description     |
| ------ | ------ | --------------- |
| `out`  | poly   | Filtered signal |

Upstream patch sums env amount, LFO, key-track into `cutoff_cv`; the
module does not do CV summation internally.

## Acceptance criteria

- [ ] Self-oscillation at max resonance tracks `cutoff` cleanly.
- [ ] Hot input (≈3.5× single source, matching VDco full mix) drives
      audible soft saturation without hard-clip artefacts.
- [ ] Variant switch produces audibly distinct character (spectrum
      analysis test: 106 shows HF rolloff + bass compression under
      resonance; 60 shows sharper peak).
- [ ] Stability: no blow-up at max resonance + max drive + full-scale
      input across a cutoff sweep.
- [ ] No allocation on audio thread (manual check; allocator trap
      once Spike 4 lands).
- [ ] `cargo clippy` and `cargo test` clean.
- [ ] Module and kernel doc comments present for `VVcf`, `VPolyVcf`,
      and the ladder kernel.
- [ ] Both variants registered via `patches-vintage`.
- [ ] `VVcf` ports mono; `VPolyVcf` ports (`in`, `cutoff_cv`, `out`)
      poly.

## Notes

- Reference: Zavalishin *The Art of VA Filter Design* ch. 5 (ladder).
- Huovilainen's per-stage tanh is a known-good alternative — pick
  whichever gives better behaviour under the project's oversampling
  policy. No oversampling is fine if tanh is tame; add 2× HB
  upsampling later if aliasing becomes audible.
- Halfband interpolator/decimator already exist in `patches-dsp` if
  oversampling is needed.
