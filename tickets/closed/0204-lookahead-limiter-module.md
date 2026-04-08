---
id: "0204"
title: Implement Limiter module in patches-modules
priority: medium
created: 2026-03-26
depends_on: "0201, 0203"
---

## Summary

Add a `Limiter` module to `patches-modules` that performs inter-sample-aware peak
limiting using the `HalfbandInterpolator` and `PeakWindow` from `patches-dsp`.

The detector runs at 2× the base rate to catch inter-sample peaks. Gain is computed
from the oversampled peak and applied to a time-aligned delayed copy of the original
signal. No downsampling occurs; the oversampled path is detector-only.

## Ports and parameters

**Inputs:**
- `in` — mono audio signal to limit

**Outputs:**
- `out` — gain-reduced mono output, time-aligned with the input

**Parameters:**
- `threshold` (linear, default 1.0) — peak level above which gain reduction is applied
- `release_ms` (f64, default 100.0) — release time in milliseconds; controls how
  quickly gain recovers after a peak. Attack is always instant.

## Signal chain (per base-rate sample)

```
input
  ├── dry_delay.push(input)
  │
  └── interpolator.process(input) → [over_a, over_b]
        peak_window.push(over_a)
        peak_window.push(over_b)
        peak = peak_window.peak()

        target_gain = if peak > threshold { threshold / peak } else { 1.0 }
        current_gain = if target_gain < current_gain {
            target_gain                                    // instant attack
        } else {
            current_gain + release_coeff * (target_gain - current_gain)
        }

        output = dry_delay.read_nearest(GROUP_DELAY_BASE_RATE) * current_gain
```

## Implementation notes

**Dry path delay:** `DelayBuffer::new(HalfbandInterpolator::GROUP_DELAY_BASE_RATE)`.
The delay must exactly match the interpolator's group delay so both signal paths refer
to the same underlying moment. `GROUP_DELAY_BASE_RATE` is 8 for the default taps.

**Release coefficient:** derived in `prepare()` from `release_ms` and the sample rate:

```rust
release_coeff = 1.0 - (-1.0 / (release_ms * 0.001 * sample_rate)).exp();
```

Recompute in `prepare()` on every plan activation; do not recompute per-sample.

**Peak window size:** `PeakWindow::new(DEFAULT_PEAK_WINDOW_LEN)` from `patches-dsp`.
This is `GROUP_DELAY_OVERSAMPLED * 2` = 32 oversampled slots, covering the full FIR
support relevant to the delayed output sample.

**Threshold safety margin:** store `threshold * 0.98` internally. The ~50 dB
stopband of the halfband filter can slightly underestimate near-Nyquist peaks; the
margin compensates without requiring 4× oversampling. Document with a comment.

**Gain initialisation:** `current_gain` starts at `1.0`; `dry_delay` is
zero-initialised by `DelayBuffer::new`. No special warm-up is needed.

**Parameter application:** apply `threshold` and `release_ms` parameter updates in
`process()` using the usual `ParameterValue` mechanism. If `release_ms` changes,
recompute `release_coeff` inline (one `exp()` call — acceptable on the audio thread
when the parameter actually changes).

## Acceptance criteria

- [ ] `Limiter` implements `Module` with correct `descriptor()`, `set_ports()`,
      `prepare()`, and `process()`.
- [ ] Registered in `patches-modules/src/lib.rs` under the name `"limiter"`.
- [ ] `patches-modules/Cargo.toml` gains a `patches-dsp` path dependency.
- [ ] `cargo clippy` and `cargo test` pass with 0 warnings across all crates.

## Out of scope

- Stereo variant (separate ticket if desired).
- Soft-knee / look-ahead parameter exposure.
- 4× oversampling option.
