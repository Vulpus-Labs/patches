---
id: "0354"
title: Drive module (multi-mode distortion)
priority: medium
created: 2026-04-12
---

## Summary

Add a `Drive` effect module offering multiple distortion algorithms selectable via an enum parameter. Signal chain includes pre/post DC blocking, asymmetric bias, post-distortion tone filtering, and dry/wet mix. Reuses `fast_tanh`, `fast_sine`, and `ToneFilter` from `patches-dsp`.

## Design

**Signal flow:**

```text
in → DC block → +bias → ×drive → waveshaper(mode) → DC block → tone filter → mix → out
```

The post-drive DC block is essential when bias is non-zero — asymmetric waveshaping introduces a DC component that must be removed.

**Distortion modes:**

| Mode       | Algorithm                                           | Character                    |
| ---------- | --------------------------------------------------- | ---------------------------- |
| `saturate` | `fast_tanh(x * drive)`                              | Warm soft clip               |
| `fold`     | `fast_sine(x * drive * 0.25)` (phase domain [0,1)) | Rich harmonics, metallic     |
| `clip`     | `(x * drive).clamp(-1.0, 1.0)`                     | Harsh hard clip              |
| `crush`    | `fast_tanh(quantize(x * drive, 6.0))`               | Gritty lo-fi digital         |

**Parameters:**

| Name    | Type  | Range        | Default    | Description                           |
| ------- | ----- | ------------ | ---------- | ------------------------------------- |
| `mode`  | enum  | saturate/fold/clip/crush | `saturate` | Distortion algorithm       |
| `drive` | float | 0.1 - 50.0  | 1.0        | Input gain before waveshaper          |
| `tone`  | float | 0.0 - 1.0   | 0.5        | Post-distortion lowpass (ToneFilter)  |
| `bias`  | float | -1.0 - 1.0  | 0.0        | DC offset before shaper (asymmetry)   |
| `mix`   | float | 0.0 - 1.0   | 1.0        | Dry/wet blend                         |

**Ports:**

| Port       | Kind | Direction | Description           |
| ---------- | ---- | --------- | --------------------- |
| `in`       | mono | input     | Audio input           |
| `drive_cv` | mono | input     | Drive modulation      |
| `out`      | mono | output    | Processed output      |

**Gain compensation:** For `saturate` mode, divide output by `fast_tanh(drive)` to keep peak level roughly consistent across drive settings.

## Acceptance criteria

- [ ] Module registered as `Drive`
- [ ] All four modes produce output in [-1, 1] for input in [-1, 1] at default drive
- [ ] `saturate` mode at drive=1.0 is near-transparent (soft knee only engages at higher levels)
- [ ] `bias` parameter produces audible even harmonics (asymmetric transfer curve)
- [ ] Post-drive DC block eliminates offset introduced by bias
- [ ] `tone` parameter sweeps from dark (200 Hz) to transparent (Nyquist)
- [ ] `mix=0.0` passes dry signal unchanged
- [ ] DC block uses one-pole HP at ~5 Hz (reuse pattern from `TapFeedbackFilter`)
- [ ] Doc comment follows module documentation standard
- [ ] `cargo test` and `cargo clippy` pass

## Notes

- Enum modes rather than separate modules: they share the same signal chain; only the nonlinear function differs. Single module keeps patches simpler and lets users A/B easily.
- `crush` mode uses a fixed 6-bit quantisation — deliberate design choice for a specific "gritty" sound. Users wanting tuneable bit depth should use the Bitcrusher module.
- Consider whether `fold` needs normalisation at extreme drive values — `fast_sine` returns [-1, 1] regardless, so it's inherently bounded.
