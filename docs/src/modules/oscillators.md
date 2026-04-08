# Oscillators

> **Source of truth:** the doc comments on each module struct in
> `patches-modules/src/` define the canonical port names, parameter
> ranges, and behaviour. This page is kept in sync with those comments.

## `Osc` — Mono oscillator

A single-voice oscillator with multiple waveform outputs.

**Parameters**

| Parameter | Type | Default | Description |
| --- | --- | --- | --- |
| `frequency` | float (v/oct) | `0.0` | Base pitch as V/oct above C0 (0.0 = C0 ≈ 16.35 Hz). Hz/kHz literals are converted to V/oct at parse time (`440Hz` → `≈4.75`); bare floats are used as V/oct directly. |
| `drift` | float | `0.0` | Slow random pitch drift amount (0 = off, 1 = max) |
| `fm_type` | string | `linear` | FM response: `linear` or `logarithmic` |

**Inputs**

| Port | Description |
| --- | --- |
| `voct` | V/oct pitch offset (added to `frequency` at runtime) |
| `fm` | Frequency modulation input |
| `pulse_width_cv` | Pulse width control for square wave (−1 to +1; maps to 0–99% duty cycle) |
| `phase_mod` | Phase modulation offset (0–1, fraction of a cycle; wraps) |

**Outputs**

| Port | Description |
| --- | --- |
| `sine` | Sine wave |
| `triangle` | Triangle wave |
| `sawtooth` | Sawtooth wave (PolyBLEP anti-aliased) |
| `square` | Square wave (PolyBLEP anti-aliased; duty cycle set by `pulse_width_cv`) |

---

## `PolyOsc` — Polyphonic oscillator

Per-voice oscillator. Each voice runs independently at the pitch set by the
`voct` poly cable.

**Parameters**

| Parameter | Type | Default | Description |
| --- | --- | --- | --- |
| `frequency` | float (v/oct) | `0.0` | Base pitch as V/oct above C0 (0.0 = C0 ≈ 16.35 Hz). Hz/kHz literals are converted at parse time; bare floats are used as V/oct directly. |
| `drift` | float | `0.0` | Slow random pitch drift amount, independent per voice (0 = off, 1 = max) |
| `fm_type` | string | `linear` | FM response: `linear` or `logarithmic` |

**Inputs**

| Port | Description |
| --- | --- |
| `voct` | Per-voice V/oct pitch offset (poly) |
| `fm` | Per-voice FM input (poly) |
| `pulse_width_cv` | Per-voice pulse width control for square wave (poly) |
| `phase_mod` | Per-voice phase modulation (poly) |

**Outputs**

| Port | Description |
| --- | --- |
| `sine` | Per-voice sine (poly) |
| `triangle` | Per-voice triangle (poly) |
| `sawtooth` | Per-voice sawtooth, PolyBLEP anti-aliased (poly) |
| `square` | Per-voice square, PolyBLEP anti-aliased (poly) |

---

## `Lfo` — Low-frequency oscillator

Multi-waveform LFO intended for modulation signals. Runs at audio rate.

`rate` is a frequency in Hz (range 0.01–20.0 Hz, default 1.0 Hz). This is
**not** a V/oct value — it is a plain frequency. `rate_cv` adds linearly to
`rate` in Hz (effective rate is clamped to 0.001–40.0 Hz).

**Parameters**

| Parameter | Type | Default | Description |
| --- | --- | --- | --- |
| `rate` | float (Hz) | `1.0` | Rate in Hz (0.01–20.0) |
| `phase_offset` | float | `0.0` | Phase offset (0–1, fraction of a cycle) |
| `mode` | string | `bipolar` | Output polarity: `bipolar` (−1 to +1), `unipolar_positive` (0 to +1), `unipolar_negative` (−1 to 0) |

**Inputs**

| Port | Description |
| --- | --- |
| `sync` | Rising edge resets phase to zero |
| `rate_cv` | Added linearly to `rate` (Hz); effective rate clamped to 0.001–40.0 Hz |

**Outputs**

| Port | Description |
| --- | --- |
| `sine` | Sine wave |
| `triangle` | Triangle wave |
| `saw_up` | Rising sawtooth |
| `saw_down` | Falling sawtooth |
| `square` | Square wave (50% duty cycle) |
| `random` | Sample-and-hold random value, updated once per cycle |
