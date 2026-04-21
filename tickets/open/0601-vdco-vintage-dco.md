---
id: "0601"
title: VDco / VPolyDco — vintage phase-locked DCO with saw/pulse/sub/noise
priority: medium
created: 2026-04-20
epic: E102
---

## Summary

Add `VDco` (mono) and `VPolyDco` (poly) to `patches-vintage`: a
Juno-style digitally-controlled oscillator. One phase accumulator
drives saw, variable-width pulse, and a ÷2 sub square, all
phase-locked. A noise source and internal mixer are folded into the
same module so the output is a single pre-mixed signal ready to feed
the HPF → VCF chain. Mono and poly variants share a single DSP core;
the wrappers differ only in port kind and per-voice state fan-out,
following the convention already used in `patches-modules`.

## Design

### Phasor

- One phase accumulator per voice (reuse `patches-dsp::phase_accumulator`).
- Pitch input: v/oct CV, matching existing oscillators.

### Waveshapes

All three waveshapes derive from the shared raw phase. Each gets its
own polyBLEP correction at its own discontinuities — **never** cascade
a BLEP-corrected saw into the pulse comparator (skews duty cycle).

- **Saw:** `phase * 2 - 1`, polyBLEP at wrap.
- **Pulse:** `phase < pwm ? +1 : -1`, polyBLEP at pwm crossing *and*
  at phase wrap (pulse flips back as phase wraps past pwm).
- **Sub:** flip-flop on phase-wrap events → ÷2 square (1 oct below),
  polyBLEP at its own transitions.
- **Noise:** white, reuse `patches-dsp::noise`.

### Mixer (internal)

- `saw_on: bool` — unity when on.
- `pulse_on: bool` — unity when on.
- `sub_level: float 0..1` — internal scale to ~1.0.
- `noise_level: float 0..1` — internal scale to ~0.5.

Gains biased, not equal. Worst-case sum ≈ 3.5× single source: sent hot
into downstream filter on purpose. No saturator — character lives in
the filter, not here.

### PWM source

PWM is a CV input, not a mode switch. Routing (manual / LFO / ENV) is
patch-level. Clamp the effective threshold to e.g. `[0.02, 0.98]` to
avoid DC-only output at the extremes.

### Parameters

| Name          | Type   | Range   | Default   | Description          |
| ------------- | ------ | ------- | --------- | -------------------- |
| `saw_on`      | bool   | —       | `true`    | Enable saw in mix    |
| `pulse_on`    | bool   | —       | `false`   | Enable pulse in mix  |
| `sub_level`   | float  | 0..1    | `0.0`     | Sub oscillator level |
| `noise_level` | float  | 0..1    | `0.0`     | Noise level          |

### Inputs

| Port   | Kind   | Description                  |
| ------ | ------ | ---------------------------- |
| `voct` | poly   | Pitch CV (1V / oct)          |
| `pwm`  | poly   | Pulse width modulation, 0..1 |

### Outputs

| Port   | Kind   | Description      |
| ------ | ------ | ---------------- |
| `out`  | poly   | Pre-mixed signal |

## Acceptance criteria

- [ ] Saw, pulse, sub individually audible with correct pitch
      relationships (sub = saw − 1 oct).
- [ ] All three waveshapes phase-lock: mixing saw + sub shows no beat
      frequency across the audible range.
- [ ] PWM sweep produces bit-accurate duty cycle (pulse comparator
      uses raw phase, not BLEP'd saw).
- [ ] Spectrum check: aliasing well below signal at top of keyboard
      range at 48 kHz.
- [ ] `cargo clippy` and `cargo test` clean.
- [ ] Module doc comments follow the project standard (Inputs /
      Outputs / Parameters tables) for both `VDco` and `VPolyDco`.
- [ ] Both variants registered via `patches-vintage`.
- [ ] `VDco` ports are mono; `VPolyDco` ports (`voct`, `pwm`, `out`)
      are poly.

## Notes

- polyBLEP helper: check `patches-dsp` for an existing implementation.
  If absent, add a `polyblep` module alongside `phase_accumulator`.
  Two-sample polyBLEP is sufficient.
- Do not model DCO reset glitch or high-pitch clock quantisation —
  out of scope.
