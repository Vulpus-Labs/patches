---
id: "E034"
title: Delay modules (Delay, StereoDelay)
status: closed
priority: medium
created: 2026-03-24
tickets:
  - "0186"
  - "0187"
  - "0188"
  - "0189"
---

## Summary

Adds two multi-tap delay modules to `patches-modules`, each with a configurable
number of taps driven by `ModuleShape::length`:

| Module         | Channels | Pan | Pingpong |
|----------------|----------|-----|----------|
| `Delay`        | Mono     | No  | No       |
| `StereoDelay`  | Stereo   | Yes | Yes      |

Both modules share a single delay buffer (or stereo pair) of 4 seconds, with all
taps reading from it at independent fractional offsets via `read_cubic`. A
`TapFeedbackFilter` helper (T-0186) provides the per-tap DC block, HF limiter,
and saturation shared by both modules.

## Per-tap controls

Every tap has:

- **`delay_ms/i`** — Int parameter [0, 2000] ms, default 500
- **`delay_cv/i`** — MonoInput; additive, clamped to [−1, 1], scaled to ±`delay_ms`
  so total addressable range is [0, 2×`delay_ms`]; max = 4000 ms = 4 s
- **`gain/i`** — Float [0, 1], default 1.0
- **`gain_cv/i`** — MonoInput; additive, result clamped to [0, 1]
- **`feedback/i`** — Float [0, 1], default 0.0
- **`fb_cv/i`** — MonoInput; additive, result clamped to [0, 1]
- **`tone/i`** — Float [0, 1], default 1.0; one-pole lowpass on tap signal (pre-gain,
  pre-feedback); 1.0 = flat, lower values roll off high frequencies
- **`drive/i`** — Float [0.1, 10.0], default 1.0; scales signal before `fast_tanh`
  in the feedback path, controlling saturation character
- **`send/i`** (or `send_l/i` + `send_r/i` for stereo) — MonoOutput; tap signal
  pre-gain, pre-pan, pre-return
- **`return/i`** (or `return_l/i` + `return_r/i`) — MonoInput; added to tap signal
  before gain/pan and before the feedback path

`StereoDelay` additionally has per tap:

- **`pan/i`** — Float [−1, 1], default 0.0
- **`pan_cv/i`** — MonoInput; additive, result clamped to [−1, 1]
- **`pingpong/i`** — Bool, default false; when true, L feedback feeds the R buffer
  and R feedback feeds the L buffer

## Module-level ports and parameters

`Delay`:
- **`in`**, **`out`** — MonoInput / MonoOutput
- **`drywet_cv`** — MonoInput (additive to `dry_wet`, result clamped to [0, 1])
- **`dry_wet`** — Float [0, 1], default 1.0

`StereoDelay`:
- **`in_l`**, **`in_r`**, **`out_l`**, **`out_r`** — MonoInput / MonoOutput
- **`drywet_cv`** — MonoInput
- **`dry_wet`** — Float [0, 1], default 1.0

## Signal flow — `Delay`

```
[write]
  write_input = in + Σ feedback[i]
  buffer.push(write_input)

[per tap i]
  eff_delay_ms = delay_ms[i] + clamp(delay_cv[i], −1, 1) * delay_ms[i]
  tap_raw      = buffer.read_cubic(eff_delay_ms / 1000 * sample_rate)
  send[i]      = tap_raw                                   // pre-gain, pre-return
  tap_sig      = tap_raw + return[i]
  tap_toned    = tone_filter[i].process(tap_sig, tone[i])  // one-pole LP
  wet_sum     += tap_toned * clamp(gain[i] + gain_cv[i], 0, 1)
  fb_sig       = tap_toned * clamp(feedback[i] + fb_cv[i], 0, 1)
  feedback[i]  = TapFeedbackFilter::process(fb_sig, drive[i])
                 // = fast_tanh(hf_limit(dc_block(fb_sig * drive[i])))

[output]
  out = lerp(in, wet_sum, clamp(dry_wet + drywet_cv, 0, 1))
```

## Signal flow — `StereoDelay`

```
[write]
  write_l = in_l + Σ fb_l[i] for !pingpong[i] + Σ fb_r[i] for pingpong[i]
  write_r = in_r + Σ fb_r[i] for !pingpong[i] + Σ fb_l[i] for pingpong[i]
  buf_l.push(write_l);  buf_r.push(write_r)

[per tap i]
  tap_l / tap_r = buf_{l,r}.read_cubic(eff_delay_samples)
  send_l[i] = tap_l;  send_r[i] = tap_r    // pre-gain, pre-pan, pre-return
  sig_l = tone_filter_l[i].process(tap_l + return_l[i], tone[i])
  sig_r = tone_filter_r[i].process(tap_r + return_r[i], tone[i])

  eff_gain = clamp(gain[i] + gain_cv[i], 0, 1)
  eff_pan  = clamp(pan[i]  + pan_cv[i], −1, 1)
  mono     = (sig_l + sig_r) * 0.5 * eff_gain   // equal-gain pan law (consistent
  wet_l   += mono * (1 − eff_pan) * 0.5          // with StereoMixer)
  wet_r   += mono * (1 + eff_pan) * 0.5

  eff_fb  = clamp(feedback[i] + fb_cv[i], 0, 1)
  fb_l[i] = TapFeedbackFilter::process(sig_l * eff_fb, drive[i])
  fb_r[i] = TapFeedbackFilter::process(sig_r * eff_fb, drive[i])

[output]
  out_l = lerp(in_l, wet_l, eff_drywet)
  out_r = lerp(in_r, wet_r, eff_drywet)
```

## Feedback filter chain (per tap, per channel)

Each `TapFeedbackFilter` applies in order:

1. **Scale by drive** — `x *= drive`
2. **DC block** — one-pole highpass at ~5 Hz:
   `y = x − x_prev + R·y_prev`,  `R ≈ 1 − 2π·5/sample_rate`
3. **HF limiter** — one-pole lowpass at ~16 kHz:
   `y = y_prev + α·(x − y_prev)`,  `α = 1 − exp(−2π·16000/sample_rate)`
4. **Saturation** — `fast_tanh(y)`

State: `x_prev`, `dc_y_prev`, `hf_y_prev`. Coefficients `R` and `α` computed once
in `prepare`.

## Buffer sizing

```
capacity = DelayBuffer::for_duration(4.0, sample_rate)
```

4 s covers the maximum modulated range of 2 × 2000 ms with 1 sample headroom on
the lower bound (clamped to ≥ 1 sample in the read).

## Pan law

Equal-gain consistent with `StereoMixer`: at pan = 0, both sides receive 0.5 of
the mono signal (−6 dBFS per side).

## Tickets

- [T-0186](../tickets/open/0186-tap-feedback-filter.md) — `TapFeedbackFilter` and `ToneFilter` helpers
- [T-0187](../tickets/open/0187-delay-module.md) — `Delay` module
- [T-0188](../tickets/open/0188-stereo-delay-module.md) — `StereoDelay` module
- [T-0189](../tickets/open/0189-delay-registry-and-tests.md) — Register both; integration tests
