# Dynamics

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

| Parameter | Type | Default | Description |
| --- | --- | --- | --- |
| `threshold` | float | `0.9` | Peak level above which gain reduction is applied (0.0–2.0). Internally multiplied by 0.98 for a small headroom margin. |
| `attack_ms` | float (ms) | `2.0` | Lookahead / attack time in milliseconds (0.1–50.0). Sets how early the gain ramp begins before a transient. |
| `release_ms` | float (ms) | `100.0` | Release time in milliseconds (1.0–5000.0). Controls how quickly the gain recovers after the peak falls. |

**Inputs**

| Port | Description |
| --- | --- |
| `in` | Audio signal to limit |

**Outputs**

| Port | Description |
| --- | --- |
| `out` | Limited output, hard-clipped to ±1.0 |

**Notes**

- The module introduces a latency of `attack_ms` plus a small FIR group delay
  (~8 samples at base rate for the interpolator). Account for this when mixing
  a limited signal against an unlimitated reference.
- All buffers are pre-allocated at `prepare` time for the maximum `attack_ms`
  value (50 ms), so changing `attack_ms` at runtime never allocates.
