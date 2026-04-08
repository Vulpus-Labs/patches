---
id: "0187"
title: Delay module (mono, N-tap)
epic: "E034"
priority: medium
created: 2026-03-24
---

## Summary

Implement the `Delay` module in `patches-modules/src/delay.rs`. It is a mono
N-tap delay line: all taps share a single `DelayBuffer` of 4 seconds; each tap
reads at an independently CV-modulatable fractional offset via `read_cubic`; each
tap's feedback signal passes through `TapFeedbackFilter` before being summed back
into the buffer input. Tap count is fixed at construction time by `ModuleShape::length`.

## Acceptance criteria

### Descriptor

- [ ] `Delay::describe(shape)` returns a `ModuleDescriptor` with:
  - Global ports: `in` (MonoInput), `out` (MonoOutput), `drywet_cv` (MonoInput)
  - Global parameter: `dry_wet` Float [0, 1] default 1.0
  - Per tap `i` in `0..shape.length`:
    - Inputs: `delay_cv/i`, `gain_cv/i`, `fb_cv/i`, `return/i` (all MonoInput)
    - Output: `send/i` (MonoOutput)
    - Parameters: `delay_ms/i` Int [0, 2000] default 500,
      `gain/i` Float [0, 1] default 1.0,
      `feedback/i` Float [0, 1] default 0.0,
      `tone/i` Float [0, 1] default 1.0,
      `drive/i` Float [0.1, 10.0] default 1.0

### State

- [ ] Single `DelayBuffer` allocated via `DelayBuffer::for_duration(4.0, sample_rate)` in `prepare`
- [ ] Per tap: `TapFeedbackFilter`, `ToneFilter`, cached parameter values (`delay_ms`,
  `gain`, `feedback`, `tone`, `drive`) updated in `update_validated_parameters`
- [ ] Per tap: `MonoInput` port fields for `delay_cv`, `gain_cv`, `fb_cv`, `return`;
  `MonoOutput` port field for `send`; all populated in `set_ports`
- [ ] Global: `MonoInput` fields for `in` and `drywet_cv`; `MonoOutput` for `out`;
  cached `dry_wet` parameter

### Processing (`process`)

- [ ] **Write**: sum `pool.read_mono(&self.in_port)` with all `feedback[i]` values
  from the previous tick, push result into buffer
- [ ] **Per tap** (in tap index order):
  1. Compute `eff_delay_ms = delay_ms[i] + clamp(pool.read_mono(&delay_cv[i]), −1, 1) * delay_ms[i]`
  2. Convert to samples: `eff_delay_ms / 1000.0 * sample_rate`, clamped to [1.0, buffer_capacity − 1]
  3. `tap_raw = buffer.read_cubic(eff_delay_samples)`
  4. Write `tap_raw` to `send[i]` output
  5. `tap_sig = tap_raw + pool.read_mono(&return[i])`
  6. `tap_toned = tone_filter[i].process(tap_sig, tone[i])`
  7. `eff_gain = (gain[i] + pool.read_mono(&gain_cv[i])).clamp(0.0, 1.0)`
  8. `wet_sum += tap_toned * eff_gain`
  9. `eff_fb = (feedback[i] + pool.read_mono(&fb_cv[i])).clamp(0.0, 1.0)`
  10. `feedback[i] = fb_filter[i].process(tap_toned * eff_fb, drive[i])`
- [ ] **Output**: `eff_dw = (dry_wet + pool.read_mono(&drywet_cv)).clamp(0.0, 1.0)`;
  `out = lerp(in_val, wet_sum, eff_dw)`; write to `out` port

### Shape validation

- [ ] `update_validated_parameters` silently clamps parameters to their declared ranges;
  no panics or unwraps
- [ ] A shape with `length = 0` is valid (produces a dry-pass module with no taps)

### Clippy / tests

- [ ] No `unwrap` or `expect` in library code
- [ ] `cargo clippy -p patches-modules` clean
- [ ] `cargo test -p patches-modules` passes (existing tests unaffected)

## Notes

`delay_ms` is an `Int` parameter so DSL users specify whole-millisecond values.
The conversion to fractional sample offset happens at audio rate in `process`, not
at parameter-update time, because `delay_cv` modulates the effective delay every
sample.

The feedback vector (`feedback: Vec<f32>`) must be pre-allocated in `prepare` to
`shape.length` elements. It is written at the end of each tick and read at the
start of the next, so it carries state across calls without any allocation on the
audio thread.

Use `read_cubic` for fractional reads. `ThiranInterp` would give smoother
pitch-modulation response but is not specified here; it can be swapped in later
without changing the module interface.
