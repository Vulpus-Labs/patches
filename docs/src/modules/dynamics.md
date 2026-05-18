# Dynamics

> **Source of truth:** the doc comments on each module struct in
> `patches-modules/src/` define the canonical port names, parameter
> ranges, and behaviour. This page is kept in sync with those comments.

## `Limiter` — Lookahead peak limiter

Transparent peak limiter with true lookahead. The `attack_ms` parameter
determines how far ahead the detector looks, giving the gain envelope time to
ramp down before a transient arrives at the output — eliminating the
overshoot that zero-latency limiters suffer.

Inter-sample peaks (peaks that fall between base-rate samples) are caught by
internally upsampling the detector path to 2× rate. The extra precision
prevents hard-clipped peaks from sneaking through at high signal levels.

The output is hard-clipped to ±1.0 as a final safety stage.

**Signal chain (per base-rate sample)**

```text
input ──► dry delay (lookahead + group delay)
      │
      └─► 2× interpolator
              │
              ▼
           peak window [t-L .. t]
              │
              ▼
           gain computation (attack / release smoothing)
              │
              ▼
           output = clamp(delayed_input × gain, −1.0, 1.0)
```

**Parameters**

| Parameter    | Type       | Default | Description                                                                                                            |
| ------------ | ---------- | ------- | ---------------------------------------------------------------------------------------------------------------------- |
| `threshold`  | float      | `0.9`   | Peak level above which gain reduction is applied (0.0–2.0). Internally multiplied by 0.98 for a small headroom margin. |
| `attack_ms`  | float (ms) | `2.0`   | Lookahead / attack time in milliseconds (0.1–50.0). Sets how early the gain ramp begins before a transient.            |
| `release_ms` | float (ms) | `100.0` | Release time in milliseconds (1.0–5000.0). Controls how quickly the gain recovers after the peak falls.                |

**Inputs**

| Port | Description           |
| ---- | --------------------- |
| `in` | Audio signal to limit |

**Outputs**

| Port  | Description                          |
| ----- | ------------------------------------ |
| `out` | Limited output, hard-clipped to ±1.0 |

**Notes**

- The module introduces a latency of `attack_ms` plus a small FIR group delay
  (~8 samples at base rate for the interpolator). Account for this when mixing
  a limited signal against an unlimitated reference.
- All buffers are pre-allocated at `prepare` time for the maximum `attack_ms`
  value (50 ms), so changing `attack_ms` at runtime never allocates.

---

## `StereoLimiter` — Stereo lookahead limiter

Stereo variant of `Limiter` with linked gain reduction across channels.
See `patches-modules/src/stereo_limiter.rs` for the current parameter set.

---

## `Compressor` — Feed-forward compressor

Feed-forward compressor with soft knee, peak / RMS detection, and a
sidechain input. When `sidechain` is unconnected the detector self-keys
from `in` (the standard hardware default — see ADR 0076).

The static gain curve is C¹-continuous across the knee region: at the
boundaries `threshold ± knee_width/2` both value and slope match the
linear / no-reduction branches, so the knee introduces no kink. A high
`ratio` value asymptotically approaches a limiter shape; no special
casing of `ratio = ∞` is required.

**Parameters**

| Parameter    | Type       | Default | Description                                                          |
| ------------ | ---------- | ------- | -------------------------------------------------------------------- |
| `threshold`  | float (dB) | `-12`   | Knee centre (−60..0 dB).                                             |
| `ratio`      | float      | `4`     | Above-knee slope (1..100). High values approach a limiter.           |
| `knee_width` | float (dB) | `6`     | Soft-knee width; `0` = hard knee (0..24 dB).                         |
| `attack`     | float (ms) | `10`    | Detector attack (0.1..1000 ms).                                      |
| `release`    | float (ms) | `100`   | Detector release (1..5000 ms).                                       |
| `makeup`     | float (dB) | `0`     | Output gain trim (−24..24 dB).                                       |
| `detect`     | enum       | `peak`  | `peak` (envelope on absolute value) or `rms` (envelope on squared).  |
| `mix`        | float      | `1`     | Dry/wet blend (0..1). `0` returns dry.                               |

**Inputs**

| Port        | Description                                                  |
| ----------- | ------------------------------------------------------------ |
| `in`        | Audio input                                                  |
| `sidechain` | External detector key. Self-keys from `in` when unconnected. |

**Outputs**

| Port  | Description       |
| ----- | ----------------- |
| `out` | Compressed audio  |

## `StereoCompressor` — True-stereo feed-forward compressor

Stereo variant of `Compressor` with linked detection: a single magnitude
derived from both channels drives one detector, and one gain reduction
value is applied identically to L and R. This preserves the stereo image
under asymmetric transients (a per-channel detector would shift the
image on panned hits).

Detector magnitude:

- **Peak:** `max(|L|, |R|)`
- **RMS:**  `sqrt((L² + R²) / 2)`

Parameters are identical to `Compressor`. The `sidechain` port is
stereo; a mono source patched in is broadcast to both channels per
ADR 0059.

**Inputs**

| Port        | Kind   | Description                                                  |
| ----------- | ------ | ------------------------------------------------------------ |
| `in`        | stereo | Audio input                                                  |
| `sidechain` | stereo | External detector key; self-keys from `in` when unconnected. |

**Outputs**

| Port  | Kind   | Description       |
| ----- | ------ | ----------------- |
| `out` | stereo | Compressed stereo |

## `Gate` — Threshold gate with hysteresis

Threshold gate with hysteresis, attack / hold / release ballistics, and a
sidechain input. The detector self-keys from `in` when `sidechain` is
unconnected (ADR 0076). Gating is binary in intent — there is no `mix`
parameter; a patch author wanting ducking-with-blend uses `Compressor`
with high ratio and `mix < 1`.

The gate opens when the rectified detector key crosses `threshold` (the
fire condition is `armed && |x| > threshold` — the threshold itself, not
`threshold - hysteresis`). Hysteresis controls *eligibility*: after a
fire the state machine disarms and only re-arms once the signal has
dropped below `threshold - hysteresis`. Once open the gate cannot close
for at least `hold` ms; after that it closes when the signal drops below
`threshold - hysteresis`. Attack and release set the ramp times of the
open/closed envelope.

**Parameters**

| Parameter    | Type       | Default | Description                                                |
| ------------ | ---------- | ------- | ---------------------------------------------------------- |
| `threshold`  | float (dB) | `-40`   | Open threshold (−80..0 dB).                                |
| `hysteresis` | float (dB) | `3`     | Re-arm band below threshold (0..24 dB).                    |
| `attack`     | float (ms) | `1`     | Open-ramp time (0.01..1000 ms).                            |
| `hold`       | float (ms) | `10`    | Minimum open time after a trigger (0..5000 ms).            |
| `release`    | float (ms) | `100`   | Close-ramp time (1..5000 ms).                              |

**Inputs**

| Port        | Description                                                  |
| ----------- | ------------------------------------------------------------ |
| `in`        | Audio input                                                  |
| `sidechain` | External detector key. Self-keys from `in` when unconnected. |

**Outputs**

| Port  | Description |
| ----- | ----------- |
| `out` | Gated audio |

## `StereoGate` — True-stereo threshold gate

Stereo variant of `Gate` with linked detection: one detector is fed by
`max(|L|, |R|)` and one gate state drives both channels. This preserves
the stereo image — per-channel detection would cause audible image shift
on asymmetric transients.

Parameters are identical to `Gate`. The `sidechain` port is stereo; a
mono source patched in is broadcast to both channels per ADR 0059.

**Inputs**

| Port        | Kind   | Description                                                  |
| ----------- | ------ | ------------------------------------------------------------ |
| `in`        | stereo | Audio input                                                  |
| `sidechain` | stereo | External detector key; self-keys from `in` when unconnected. |

**Outputs**

| Port  | Kind   | Description  |
| ----- | ------ | ------------ |
| `out` | stereo | Gated stereo |

## `Bitcrusher` — Bit-depth and sample-rate reduction

Quantises samples to a configurable bit depth and decimates the sample
rate. See `patches-modules/src/bitcrusher.rs`.

## `Drive` — Nonlinear waveshaper

Soft-clip saturation / overdrive. See `patches-modules/src/drive.rs`.

## `TransientShaper` — Attack/sustain emphasis

Envelope-follower-based transient emphasis and suppression. See
`patches-modules/src/transient_shaper.rs`.
