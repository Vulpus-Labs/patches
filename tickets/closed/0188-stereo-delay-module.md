---
id: "0188"
title: StereoDelay module (stereo, N-tap, pingpong)
epic: "E034"
priority: medium
created: 2026-03-24
---

## Summary

Implement the `StereoDelay` module in `patches-modules/src/stereo_delay.rs`. It
mirrors `Delay` but operates on two `DelayBuffer`s (L and R). Each tap has an
independent pan position and a `pingpong` flag that cross-routes its feedback
signal from L into the R buffer and vice versa.

## Acceptance criteria

### Descriptor

- [ ] `StereoDelay::describe(shape)` returns a `ModuleDescriptor` with:
  - Global ports: `in_l`, `in_r` (MonoInput), `out_l`, `out_r` (MonoOutput),
    `drywet_cv` (MonoInput)
  - Global parameter: `dry_wet` Float [0, 1] default 1.0
  - Per tap `i` in `0..shape.length`:
    - Inputs: `delay_cv/i`, `gain_cv/i`, `fb_cv/i`, `pan_cv/i`,
      `return_l/i`, `return_r/i` (all MonoInput)
    - Outputs: `send_l/i`, `send_r/i` (MonoOutput)
    - Parameters: `delay_ms/i` Int [0, 2000] default 500,
      `gain/i` Float [0, 1] default 1.0,
      `feedback/i` Float [0, 1] default 0.0,
      `tone/i` Float [0, 1] default 1.0,
      `drive/i` Float [0.1, 10.0] default 1.0,
      `pan/i` Float [−1, 1] default 0.0,
      `pingpong/i` Bool default false

### State

- [ ] Two `DelayBuffer`s (`buf_l`, `buf_r`) allocated via
  `DelayBuffer::for_duration(4.0, sample_rate)` in `prepare`
- [ ] Per tap: `TapFeedbackFilter` × 2 (one per channel), `ToneFilter` × 2,
  cached parameter values; `MonoInput`/`MonoOutput` port fields set in `set_ports`
- [ ] Per tap: `fb_l: f32` and `fb_r: f32` carried between ticks (pre-allocated
  in `prepare` to `shape.length`; no audio-thread allocation)

### Processing (`process`)

- [ ] **Write**:
  ```
  write_l = in_l + Σ fb_l[i] for !pingpong[i] + Σ fb_r[i] for pingpong[i]
  write_r = in_r + Σ fb_r[i] for !pingpong[i] + Σ fb_l[i] for pingpong[i]
  buf_l.push(write_l);  buf_r.push(write_r)
  ```
- [ ] **Per tap** (in tap index order):
  1. Compute `eff_delay_samples` using same formula as `Delay` (T-0187)
  2. `tap_l = buf_l.read_cubic(eff_delay_samples)`;
     `tap_r = buf_r.read_cubic(eff_delay_samples)`
  3. Write `tap_l` → `send_l[i]`, `tap_r` → `send_r[i]`
  4. `sig_l = tone_filter_l[i].process(tap_l + return_l[i], tone[i])`
     `sig_r = tone_filter_r[i].process(tap_r + return_r[i], tone[i])`
  5. `eff_gain = (gain[i] + gain_cv[i]).clamp(0.0, 1.0)`
     `eff_pan  = (pan[i]  + pan_cv[i]).clamp(−1.0, 1.0)`
     `mono     = (sig_l + sig_r) * 0.5 * eff_gain`
     `wet_l   += mono * (1.0 − eff_pan) * 0.5`
     `wet_r   += mono * (1.0 + eff_pan) * 0.5`
  6. `eff_fb = (feedback[i] + fb_cv[i]).clamp(0.0, 1.0)`
     `fb_l[i] = fb_filter_l[i].process(sig_l * eff_fb, drive[i])`
     `fb_r[i] = fb_filter_r[i].process(sig_r * eff_fb, drive[i])`
- [ ] **Output**: dry/wet lerp on both channels independently

### Shape / robustness

- [ ] `length = 0` is valid (stereo dry pass)
- [ ] No `unwrap` / `expect` in library code; clippy clean

## Notes

`fb_l` and `fb_r` arrays are written at the end of each tick and consumed at the
start of the next write phase. The one-tick lag is intentional and consistent with
the existing 1-sample cable delay model.

The pan law (equal-gain, −6 dBFS at centre) is consistent with `StereoMixer`
(E029). The sum-to-mono approach (`(sig_l + sig_r) * 0.5`) means a stereo ping-pong
pattern is folded to mono before panning; this is a deliberate simplification. If
preserving inter-tap stereo imaging becomes important, the alternative is to apply
the pan matrix to L and R independently, routing the result via a constant-power
law — defer to a follow-up ticket if needed.

The `pingpong` flag is per tap, so a patch can mix independent and ping-pong taps
on the same module instance without restriction.
