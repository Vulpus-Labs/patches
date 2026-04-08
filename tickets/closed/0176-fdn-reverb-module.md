---
id: "0176"
title: Implement `FdnReverb` module
priority: medium
created: 2026-03-22
epic: "E030"
depends_on: ["0175"]
---

## Summary

Implements the `FdnReverb` struct in `patches-modules/src/fdn_reverb.rs`: an
8-line Feedback Delay Network reverb with Hadamard mixing matrix, per-line
high-shelf absorption (MonoBiquad), Thiran all-pass interpolated and
LFO-modulated delay reads, and stereo output derived from orthogonal output
gain vectors.

## Acceptance criteria

### Module descriptor

- [ ] Name: `"FdnReverb"`
- [ ] Inputs (all Mono):
  - `"in_l"` (index 0)
  - `"in_r"` (index 1)
  - `"size_cv"` (index 2)
  - `"brightness_cv"` (index 3)
- [ ] Outputs (all Mono):
  - `"out_l"` (index 0)
  - `"out_r"` (index 1)
- [ ] Parameters:
  - `"size"` — Float, [0, 1], default 0.5
  - `"brightness"` — Float, [0, 1], default 0.5
  - `"character"` — Enum, variants `["plate", "room", "hall", "chamber",
    "cathedral"]`, default `"hall"`

### Internal structure

- [ ] Eight `DelayBuffer` instances (allocated in `prepare`).
- [ ] Eight `ThiranInterp` instances (one per delay line).
- [ ] Eight `MonoBiquad` instances for absorption (one per delay line).
- [ ] Eight LFO phases (`f32`) with per-character fixed rate and depth; initial
      phases evenly spaced: `phase[i] = i as f32 / 8.0`.
- [ ] One `DelayBuffer` for pre-delay (allocated in `prepare`).
- [ ] Connectivity flags set in `set_connectivity`:
      `stereo_in: bool`, `stereo_out: bool`.

### `prepare`

- [ ] Allocates all delay line buffers using `DelayBuffer::for_duration` with a
      maximum duration of `max_delay_ms(character) / 1000.0` seconds at the
      given sample rate, where `max_delay_ms` is the largest base delay
      multiplied by `max_scale(character)` plus the maximum LFO depth.
- [ ] Allocates the pre-delay buffer for `max_pre_delay_ms(character) / 1000.0`.
- [ ] Resets all `ThiranInterp` states.
- [ ] Computes initial absorption coefficients from default parameter values.

### Character archetypes

Each archetype defines the following constants (exact values to be tuned by
listening; these are the design targets):

| Archetype   | Scale range     | LFO rate (Hz) | LFO depth (ms) | Max pre-delay (ms) |
|-------------|-----------------|---------------|----------------|--------------------|
| `plate`     | 0.08 → 0.40     | 0.27          | 0.3            | 10                 |
| `room`      | 0.10 → 0.80     | 0.15          | 0.8            | 25                 |
| `chamber`   | 0.15 → 0.60     | 0.20          | 0.5            | 20                 |
| `hall`      | 0.20 → 1.20     | 0.10          | 1.2            | 50                 |
| `cathedral` | 0.40 → 2.50     | 0.06          | 2.0            | 80                 |

Scale is mapped from size [0, 1] via exponential interpolation:
`scale = min_scale * (max_scale / min_scale)^size`

Brightness [0, 1] is linearly interpolated to `(crossover_hz, lf_hf_ratio)`:

| Archetype   | crossover at b=0 → b=1 | lf_hf_ratio at b=0 → b=1 |
|-------------|------------------------|--------------------------|
| `plate`     | 2000 → 8000 Hz         | 1.5 → 1.1                |
| `room`      | 500 → 2500 Hz          | 3.0 → 1.5                |
| `chamber`   | 800 → 6000 Hz          | 2.5 → 1.2                |
| `hall`      | 300 → 2000 Hz          | 5.0 → 2.0                |
| `cathedral` | 200 → 1500 Hz          | 8.0 → 3.0                |

Base delay line lengths in ms (shared across all archetypes, before scaling):
`[29.7, 37.1, 41.1, 43.7, 53.3, 59.7, 67.1, 79.3]`

### Absorption coefficient computation

For delay line `i` of length `L_i` samples, given `rt60_lf` and `rt60_hf`
(both derived from size and character; start with a simple linear map):

```
g_lf_i = 10.0_f32.powf(-3.0 * L_i / (rt60_lf * sample_rate))
g_hf_i = 10.0_f32.powf(-3.0 * L_i / (rt60_hf * sample_rate))
```

`rt60_hf = rt60_lf / lf_hf_ratio`; `rt60_lf` maps size [0, 1] per character:

| Archetype   | rt60_lf at size=0 → size=1 |
|-------------|----------------------------|
| `plate`     | 0.3 → 1.5 s                |
| `room`      | 0.4 → 2.5 s                |
| `chamber`   | 0.3 → 2.0 s                |
| `hall`      | 0.8 → 5.0 s                |
| `cathedral` | 1.5 → 8.0 s                |

Set the MonoBiquad for line `i` to a high-shelf with DC gain `g_lf_i`, HF gain
`g_hf_i`, and crossover `crossover_hz` (from brightness + character).  The
shelf coefficients follow the Audio EQ Cookbook (high-shelf with S=1).
Recompute on every call to `process` where size or brightness CV has changed
by more than a threshold (≥ 1/COEFF_UPDATE_INTERVAL samples, same cadence as
existing biquad modules).

### Per-sample signal flow

```
1.  effective_size       = clamp(size_param + size_cv, 0.0, 1.0)
    effective_brightness = clamp(brightness_param + brightness_cv, 0.0, 1.0)

2.  pre_delay.push(in_l)
    x_l = pre_delay.read_nearest(pre_delay_samples)
    x_r = if stereo_in { pre_delay_r.push(in_r); pre_delay_r.read_nearest(...) }
           else        { x_l }   // copy left when in_r not connected

3.  For i in 0..8:
        lfo_val = sin(lfo_phase[i])
        lfo_phase[i] = (lfo_phase[i] + lfo_phase_inc[i]) % TAU
        offset_i = base_length_samples[i] + lfo_depth_samples * lfo_val
        raw[i] = thiran[i].read(&delays[i], offset_i)

4.  For i in 0..8:
        damp[i] = absorption[i].process(raw[i])

5.  f = hadamard8(damp)   // FWHT, normalised by 1/√8

6.  For i in 0..8:
        delays[i].push(INPUT_GAIN * lerp(x_l, x_r, injection_pan[i]) + f[i])
    where injection_pan splits L/R evenly: first 4 lines take x_l, last 4 take x_r
    (or interleaved — chosen to match output gain orthogonality)

7.  if stereo_out:
        out_l = OUTPUT_SCALE * dot(OUT_GAINS_L, damp)
        out_r = OUTPUT_SCALE * dot(OUT_GAINS_R, damp)
    else:
        out_l = OUTPUT_SCALE * sum(damp) / √8
```

Output gain vectors (orthogonal, alternating signs, 1/√8 normalised):
```
OUT_GAINS_L = [+1, -1, +1, -1, +1, -1, +1, -1] * (1/√8)
OUT_GAINS_R = [+1, +1, -1, -1, +1, +1, -1, -1] * (1/√8)
```
`INPUT_GAIN = 1.0 / √8`

### Hadamard transform

```rust
fn hadamard8(mut x: [f32; 8]) -> [f32; 8] {
    // Three butterfly passes (FWHT for N=8).
    for step in [4_usize, 2, 1] {
        let mut i = 0;
        while i < 8 {
            for j in i..i + step {
                let a = x[j];
                let b = x[j + step];
                x[j]        = a + b;
                x[j + step] = a - b;
            }
            i += step * 2;
        }
    }
    // Normalise by 1/√8 to preserve energy.
    const NORM: f32 = 1.0 / 2.8284271; // 1/√8
    x.map(|v| v * NORM)
}
```

### Connectivity

- [ ] `set_connectivity` stores `stereo_in` and `stereo_out` flags; allocates a
      second pre-delay buffer if transitioning to stereo-in mode (or on first
      `prepare` when stereo-in is already connected).
- [ ] `process` skips computing `out_r` when `!stereo_out`.

### Tests

- [ ] Descriptor has correct port names, counts, and parameter ranges.
- [ ] Impulse through all five characters produces non-zero, finite output that
      decays to near-zero within twice the expected RT60.
- [ ] DC input (constant signal) passes through at approximately unity gain
      after settling (tolerance 5%).
- [ ] Mono mode (`out_r` disconnected): `out_r` buffer unchanged.
- [ ] Stereo mode with mono input: `out_l` and `out_r` differ (decorrelation).
- [ ] `cargo clippy`, `cargo test` pass with no new warnings.

## Notes

Absorption coefficients must be recomputed whenever size or brightness change.
To avoid per-sample coefficient recalculation when CV is slowly varying, use
the same `COEFF_UPDATE_INTERVAL = 32` cadence already used in `MonoBiquad`.

The exact constant values in the character tables are design targets, not
commitments; they will need listening-test calibration.  Keep them in a single
named constant block so adjustment is a one-place change.

For the pre-delay in stereo-in mode a second `DelayBuffer` is needed (one per
channel).  In mono-in mode only one is needed.  Allocate both in `prepare` when
`stereo_in` is true to avoid allocation on the audio thread later; accept the
small memory overhead in mono-in mode.
