# Utilities

> **Source of truth:** the doc comments on each module struct in
> `patches-modules/src/` define the canonical port names, parameter
> ranges, and behaviour. This page is kept in sync with those comments.

## `Glide` — Portamento / pitch smoothing

Smooths a stepped input signal toward its target using a one-pole low-pass
filter. Because V/oct is a log-frequency scale, linear interpolation in V/oct
space gives a perceptually constant-ratio (equal-interval) glide.

**Parameters**

| Parameter | Type | Default | Description |
| --- | --- | --- | --- |
| `glide_ms` | float (ms) | `100.0` | Glide time in milliseconds (0–10000) |

**Inputs**

| Port | Description |
| --- | --- |
| `in` | Input signal (typically V/oct) |

**Outputs**

| Port | Description |
| --- | --- |
| `out` | Smoothed output |

---

## `Tuner` — Pitch offset

Transposes a V/oct signal by a fixed interval expressed as octaves, semitones,
and cents. All three parameters are additive:

```text
out = in + octave + semi/12 + cent/1200
```

Setting all parameters to zero passes the signal through unchanged.

**Parameters**

| Parameter | Type | Default | Description |
| --- | --- | --- | --- |
| `octave` | int | `0` | Octave shift (−8 to +8) |
| `semi` | int | `0` | Semitone shift (−12 to +12) |
| `cent` | int | `0` | Fine-tune in cents (−100 to +100; 100 cents = 1 semitone) |

**Inputs**

| Port | Description |
| --- | --- |
| `in` | V/oct input |

**Outputs**

| Port | Description |
| --- | --- |
| `out` | Transposed V/oct |

---

## `Sah` — Sample and hold

Latches the `in` signal on each rising edge of `trig` (threshold 0.5) and holds
the sampled value on `out` until the next trigger. The held value is initialised
to 0.0 before the first trigger.

**Inputs**

| Port | Description |
| --- | --- |
| `in` | Signal to sample |
| `trig` | Trigger input; latch fires on the ≥ 0.5 rising edge |

**Outputs**

| Port | Description |
| --- | --- |
| `out` | Held output value |

---

## `PolySah` — Polyphonic sample and hold

Polyphonic variant of `Sah`. A single mono `trig` latches all 16 voice channels
simultaneously on each rising edge (threshold 0.5).

**Inputs**

| Port | Description |
| --- | --- |
| `in` | Polyphonic signal to sample (16 voices) |
| `trig` | Mono trigger; all voices are latched together |

**Outputs**

| Port | Description |
| --- | --- |
| `out` | Polyphonic held output (16 voices) |

---

## `Quant` — V/oct quantiser

Snaps a continuous V/oct signal to the nearest note in a user-supplied semitone
set. The formula applied before quantisation is:

```text
quantised_input = centre + in × scale
```

Emits a one-sample pulse on `trig_out` whenever the quantised pitch changes.

**Parameters**

| Parameter | Type | Default | Description |
| --- | --- | --- | --- |
| `notes` | array (up to 12) | `["0"]` | Semitone offsets within an octave (0–11). Values are parsed as integers. |
| `centre` | float (V/oct) | `0.0` | DC offset added to the input before quantisation (−4 to +4) |
| `scale` | float | `1.0` | Gain applied to `in` before quantisation (−4 to +4) |

**Inputs**

| Port | Description |
| --- | --- |
| `in` | Continuous V/oct input |

**Outputs**

| Port | Description |
| --- | --- |
| `out` | Quantised V/oct output |
| `trig_out` | One-sample pulse (1.0) on each pitch change, otherwise 0.0 |

---

## `PolyQuant` — Polyphonic V/oct quantiser

Applies the same quantisation logic as `Quant` independently to each of the 16
polyphonic voices. All voices share the same `notes`, `centre`, and `scale`
parameters. Each voice has its own `trig_out` channel that fires independently
when that voice's quantised pitch changes.

**Parameters**

| Parameter | Type | Default | Description |
| --- | --- | --- | --- |
| `notes` | array (up to 12) | `["0"]` | Semitone offsets within an octave (0–11) |
| `centre` | float (V/oct) | `0.0` | DC offset added to each voice before quantisation (−4 to +4) |
| `scale` | float | `1.0` | Gain applied to each voice before quantisation (−4 to +4) |

**Inputs**

| Port | Description |
| --- | --- |
| `in` | Polyphonic V/oct input (16 voices) |

**Outputs**

| Port | Description |
| --- | --- |
| `out` | Polyphonic quantised V/oct output (16 voices) |
| `trig_out` | Per-voice one-sample pulse on pitch change (16 voices) |

---

## `RingMod` — Diode ring modulator

Analog diode-bridge ring modulator model (Julian Parker, DAFx-11). Produces
sum and difference frequencies. The `drive` parameter sets the operating point
on the diode I–V curve: low drive is near-ideal multiplication; higher drive
introduces harmonic colouring.

**Parameters**

| Parameter | Type | Default | Description |
| --- | --- | --- | --- |
| `drive` | float (dB) | `1.0` | Diode operating point in dB (0.2–20.0); low = linear, high = saturated |

**Inputs**

| Port | Description |
| --- | --- |
| `signal` | Audio signal to modulate |
| `carrier` | Carrier / modulator signal |

**Outputs**

| Port | Description |
| --- | --- |
| `out` | Ring-modulated output |
