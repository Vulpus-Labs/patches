# Oscillators

> **Source of truth:** the doc comments on each module struct in
> `patches-modules/src/` define the canonical port names, parameter
> ranges, and behaviour. This page is kept in sync with those comments.

## `Osc` — Mono oscillator

A single-voice oscillator with multiple waveform outputs.

**Parameters**

| Parameter   | Type          | Default  | Description                                                                                                                                                           |
| ----------- | ------------- | -------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `frequency` | float (v/oct) | `0.0`    | Base pitch as V/oct above C0 (0.0 = C0 ≈ 16.35 Hz). Hz/kHz literals are converted to V/oct at parse time (`440Hz` → `≈4.75`); bare floats are used as V/oct directly. |
| `drift`     | float         | `0.0`    | Slow random pitch drift amount (0 = off, 1 = max)                                                                                                                     |
| `fm_type`   | string        | `linear` | FM response: `linear` or `logarithmic`                                                                                                                                |

**Inputs**

| Port             | Description                                                                                 |
| ---------------- | ------------------------------------------------------------------------------------------- |
| `voct`           | V/oct pitch offset (added to `frequency` at runtime)                                        |
| `fm`             | Frequency modulation input                                                                  |
| `pulse_width_cv` | Pulse width control for square wave (−1 to +1; maps to 0–99% duty cycle)                    |
| `phase_mod`      | Phase modulation offset (0–1, fraction of a cycle; wraps)                                   |
| `sync`           | Sub-sample hard-sync trigger: phase reset with PolyBLEP correction on saw/square            |

**Outputs**

| Port        | Description                                                                       |
| ----------- | --------------------------------------------------------------------------------- |
| `sine`      | Sine wave                                                                         |
| `triangle`  | Triangle wave                                                                     |
| `sawtooth`  | Sawtooth wave (PolyBLEP anti-aliased)                                             |
| `square`    | Square wave (PolyBLEP anti-aliased; duty cycle set by `pulse_width_cv`)           |
| `reset_out` | Trigger emitting the sub-sample fractional position of each phase wrap            |

---

## `PolyOsc` — Polyphonic oscillator

Per-voice oscillator. Each voice runs independently at the pitch set by the
`voct` poly cable.

**Parameters**

| Parameter   | Type          | Default  | Description                                                                                                                              |
| ----------- | ------------- | -------- | ---------------------------------------------------------------------------------------------------------------------------------------- |
| `frequency` | float (v/oct) | `0.0`    | Base pitch as V/oct above C0 (0.0 = C0 ≈ 16.35 Hz). Hz/kHz literals are converted at parse time; bare floats are used as V/oct directly. |
| `drift`     | float         | `0.0`    | Slow random pitch drift amount, independent per voice (0 = off, 1 = max)                                                                 |
| `fm_type`   | string        | `linear` | FM response: `linear` or `logarithmic`                                                                                                   |

**Inputs**

| Port             | Description                                          |
| ---------------- | ---------------------------------------------------- |
| `voct`           | Per-voice V/oct pitch offset (poly)                  |
| `fm`             | Per-voice FM input (poly)                            |
| `pulse_width_cv` | Per-voice pulse width control for square wave (poly) |
| `phase_mod`      | Per-voice phase modulation (poly)                    |

**Outputs**

| Port       | Description                                      |
| ---------- | ------------------------------------------------ |
| `sine`     | Per-voice sine (poly)                            |
| `triangle` | Per-voice triangle (poly)                        |
| `sawtooth` | Per-voice sawtooth, PolyBLEP anti-aliased (poly) |
| `square`   | Per-voice square, PolyBLEP anti-aliased (poly)   |

---

## Supersaw — `HyperSaw` and `PolyHyperSaw`

A *supersaw* (JP-8000 style) is a stack of detuned sawtooth copies summed to
one voice. `HyperSaw`/`PolyHyperSaw` use **9 copies** — one centre saw plus four
pairs of side saws, detuned symmetrically below and above the centre pitch. The
copies run at slightly different frequencies, so they slowly drift in and out of
phase with one another; that continuous beating is the wide, animated character
you cannot get from a single saw or a static wavetable.

Three controls shape the ensemble:

- **`spread`** — the detune width. `0` collapses every copy onto the centre
  pitch (a plain unison saw, no beating); `1` pushes the outermost pair to
  ±50 cents. The pairs are spaced nonlinearly (inner pairs detuned much less
  than the outer ones).
- **`density`** — how many of the four side pairs are active, faded in
  inner→outer. The fade is loudness-normalised, so turning density up thickens
  the sound without a level jump.
- **`mix`** — the centre↔side balance. `0` is the clean centre saw alone; `1` is
  the full stack with the centre and sides together. The summed output is
  normalised to stay within `[-1, 1]`.

**FM here is vibrato, not timbre.** Unlike `Osc`/`PolyOsc`, which recompute pitch
every sample, the supersaw recomputes all 144 increments (9 copies × 16 voices)
only at the control-rate interval — recomputing them per sample is not
affordable. Pitch and FM are therefore sampled at control rate: `fm` is a
vibrato input, and there is **no hard-sync and no phase modulation** (syncing
would re-align the copies and kill the beating that is the whole point). See
**ADR 0078** for the design and the fixed-point, autovectorised kernel behind it.

The copies' start phases are randomised once at construction, so there is no
thin attack and no comb-filtered coincident-wrap aliasing.

A worked patch using `PolyHyperSaw` into a filter and amp lives in
[`examples/synths/hypersaw_lead.patches`](https://github.com/Vulpus-Labs/patches/blob/main/examples/synths/hypersaw_lead.patches).

### `HyperSaw` — Mono supersaw

A single detuned-saw ensemble. Runs the 16-voice kernel and reads one voice;
mono is not the performance case.

**Parameters**

| Parameter   | Type          | Default  | Description                                                  |
| ----------- | ------------- | -------- | ------------------------------------------------------------ |
| `frequency` | float (v/oct) | `0.0`    | Base pitch as V/oct above C0 (0.0 = C0 ≈ 16.35 Hz)           |
| `fm_type`   | string        | `linear` | FM response: `linear` or `logarithmic`                       |
| `spread`    | float         | `0.3`    | Detune width (0 = unison, 1 = ±50 cents at the outer pair)   |
| `density`   | float         | `1.0`    | Active side pairs, faded in inner→outer (×4 pairs)           |
| `mix`       | float         | `0.7`    | Centre↔side balance (0 = clean centre saw, 1 = full stack)   |

**Inputs**

| Port         | Description                                            |
| ------------ | ------------------------------------------------------ |
| `voct`       | V/oct pitch offset (added to `frequency` at runtime)   |
| `fm`         | Frequency modulation (control-rate vibrato)            |
| `spread_cv`  | Adds to `spread`                                       |
| `density_cv` | Adds to `density`                                      |
| `mix_cv`     | Adds to `mix`                                          |

**Outputs**

| Port  | Description                                         |
| ----- | --------------------------------------------------- |
| `out` | Summed detuned-saw ensemble (PolyBLEP anti-aliased) |

---

### `PolyHyperSaw` — Polyphonic supersaw

The 16-voice counterpart: one detuned ensemble per voice, driven by per-voice
`voct`/`fm`. `spread`/`density`/`mix` CV are **mono (shared across voices)** so
the detune ratios are computed once per control interval and reused for every
voice; per-voice spread is deferred.

**Parameters**

Same as `HyperSaw` (`frequency`, `fm_type`, `spread`, `density`, `mix`).

**Inputs**

| Port         | Description                                       |
| ------------ | ------------------------------------------------- |
| `voct`       | Per-voice V/oct pitch offset (poly)               |
| `fm`         | Per-voice FM input (poly; control-rate vibrato)   |
| `spread_cv`  | Adds to `spread`, shared across voices (mono)     |
| `density_cv` | Adds to `density`, shared across voices (mono)    |
| `mix_cv`     | Adds to `mix`, shared across voices (mono)        |

**Outputs**

| Port  | Description                                                  |
| ----- | ------------------------------------------------------------ |
| `out` | Per-voice detuned-saw ensemble, PolyBLEP anti-aliased (poly) |

---

## `Lfo` — Low-frequency oscillator

Multi-waveform LFO intended for modulation signals. Runs at audio rate.

`rate` is a frequency in Hz (range 0.01–20.0 Hz, default 1.0 Hz). This is
**not** a V/oct value — it is a plain frequency. `rate_cv` adds linearly to
`rate` in Hz (effective rate is clamped to 0.001–40.0 Hz).

**Parameters**

| Parameter      | Type       | Default   | Description                                                                                         |
| -------------- | ---------- | --------- | --------------------------------------------------------------------------------------------------- |
| `rate`         | float (Hz) | `1.0`     | Rate in Hz (0.01–20.0)                                                                              |
| `phase_offset` | float      | `0.0`     | Phase offset (0–1, fraction of a cycle)                                                             |
| `mode`         | string     | `bipolar` | Output polarity: `bipolar` (−1 to +1), `unipolar_positive` (0 to +1), `unipolar_negative` (−1 to 0) |

**Inputs**

| Port      | Description                                                                  |
| --------- | ---------------------------------------------------------------------------- |
| `sync`    | Sub-sample phase reset trigger                                               |
| `rate_cv` | Added linearly to `rate` (Hz); effective rate clamped to 0.001–40.0 Hz       |
| `sync_ms` | When connected, overrides `rate` with this value interpreted as period in ms |

**Outputs**

| Port        | Description                                                                       |
| ----------- | --------------------------------------------------------------------------------- |
| `sine`      | Sine wave                                                                         |
| `triangle`  | Triangle wave                                                                     |
| `saw_up`    | Rising sawtooth                                                                   |
| `saw_down`  | Falling sawtooth                                                                  |
| `square`    | Square wave (50% duty cycle)                                                      |
| `random`    | Sample-and-hold random value, updated once per cycle                              |
| `reset_out` | Trigger emitting the sub-sample fractional position of each phase wrap            |
