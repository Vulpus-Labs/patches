# Filters

All filter modules implement a resonant biquad (Transposed Direct Form II).

> **Note:** The legacy `Filter` module name is no longer supported. Use
> `Lowpass` instead.

---

## `Lowpass` / `Highpass`

Mono resonant filters.

**Parameters**

| Parameter | Type | Default | Description |
| --- | --- | --- | --- |
| `cutoff` | float (V/oct) | `6.0` | Cutoff as V/oct above C0: `0.0` = C0 (≈16 Hz), `4.0` = C4 (≈262 Hz), `6.0` = C6 (≈1047 Hz). `Hz`/`kHz` literals are accepted and converted at parse time. |
| `resonance` | float | `0.0` | Resonance (0–1; approaching 1 = self-oscillation) |
| `saturate` | bool | `false` | Apply soft-clip saturation in the feedback path |

**Inputs**

| Port | Description |
| --- | --- |
| `in` | Audio input |
| `voct` | V/oct offset added to `cutoff`; 1.0 = +1 octave |
| `fm` | FM sweep: ±1 sweeps ±4 octaves around `cutoff` |
| `resonance_cv` | Added to `resonance` |

**Outputs**

| Port | Description |
| --- | --- |
| `out` | Filtered audio |

---

## `Bandpass`

Mono resonant bandpass filter.

**Parameters**

| Parameter | Type | Default | Description |
| --- | --- | --- | --- |
| `center` | float (V/oct) | `6.0` | Centre frequency as V/oct above C0: `0.0` = C0 (≈16 Hz), `4.0` = C4 (≈262 Hz), `6.0` = C6 (≈1047 Hz). `Hz`/`kHz` literals are accepted and converted at parse time. |
| `bandwidth_q` | float | `1.0` | Bandwidth Q (0.1–20; higher = narrower band) |
| `saturate` | bool | `false` | Apply soft-clip saturation in the feedback path |

**Inputs**

| Port | Description |
| --- | --- |
| `in` | Audio input |
| `voct` | V/oct offset added to `center`; 1.0 = +1 octave |
| `fm` | FM sweep: ±1 sweeps ±4 octaves around `center` |
| `resonance_cv` | Added to `bandwidth_q` |

**Outputs**

| Port | Description |
| --- | --- |
| `out` | Filtered audio |

---

## `PolyLowpass` / `PolyHighpass`

Per-voice versions of `Lowpass` / `Highpass`. Each voice maintains
independent biquad state. Same parameters as their mono counterparts.
CV inputs carry poly cables; modulation is applied per-voice.

**Inputs**

| Port | Description |
| --- | --- |
| `in` | Per-voice audio (poly) |
| `voct` | Per-voice V/oct offset added to `cutoff`; 1.0 = +1 octave (poly) |
| `fm` | Per-voice FM sweep: ±1 sweeps ±4 octaves around `cutoff` (poly) |
| `resonance_cv` | Per-voice resonance modulation (poly) |

**Outputs**

| Port | Description |
| --- | --- |
| `out` | Per-voice filtered audio (poly) |

---

---

## `Svf`

Mono State Variable Filter (Chamberlin topology). Produces lowpass, highpass,
and bandpass outputs simultaneously from a single audio input. All three outputs
can be used at once — they share the same two integrator state variables and cost
no extra computation per connected output.

Unlike the biquad filters above, `Svf` is capable of **self-oscillation**: at
high `q` values the filter will sustain a sine wave at the cutoff frequency
without any audio input. Noise or other signals act as an exciter to kick the
filter into ringing.

**Parameters**

| Parameter | Type | Default | Description |
| --- | --- | --- | --- |
| `cutoff` | float (V/oct) | `6.0` | Cutoff as V/oct above C0: `0.0` = C0 (≈16 Hz), `4.0` = C4 (≈262 Hz), `6.0` = C6 (≈1047 Hz). `Hz`/`kHz` literals are accepted and converted at parse time. |
| `q` | float | `0.0` | Resonance (0–1). The mapping is exponential: values below 0.5 give moderate resonance; above 0.9 the filter enters self-oscillation territory (Q ≈ 70–100). |

**Inputs**

| Port | Description |
| --- | --- |
| `in` | Audio input |
| `voct` | V/oct offset added to `cutoff`; 1.0 = +1 octave |
| `fm` | FM sweep: ±1 sweeps ±4 octaves around `cutoff` |
| `q_cv` | Added to `q` before clamping to [0, 1] |

**Outputs**

| Port | Description |
| --- | --- |
| `lowpass` | Lowpass output |
| `highpass` | Highpass output |
| `bandpass` | Bandpass output |

> **Warning:** At `q` values above ≈ 0.95 the filter self-oscillates. Loud
> audio input at those settings can drive the output to clip. Scale the output
> down or use a VCA after the filter if this is a concern.

---

## `PolySvf`

Per-voice version of `Svf`. Each voice maintains independent integrator state.
All parameters and CV semantics are identical to `Svf`; all ports carry poly
cables.

**Parameters**

Same as `Svf`: `cutoff` and `q`.

**Inputs**

| Port | Description |
| --- | --- |
| `in` | Per-voice audio (poly) |
| `voct` | Per-voice V/oct offset added to `cutoff` (poly) |
| `fm` | Per-voice FM sweep: ±1 sweeps ±4 octaves around `cutoff` (poly) |
| `q_cv` | Per-voice additive Q offset, clamped to [0, 1] (poly) |

**Outputs**

| Port | Description |
| --- | --- |
| `lowpass` | Per-voice lowpass output (poly) |
| `highpass` | Per-voice highpass output (poly) |
| `bandpass` | Per-voice bandpass output (poly) |

---

## `PolyBandpass`

Per-voice version of `Bandpass`. Same parameters as `Bandpass`.

**Inputs**

| Port | Description |
| --- | --- |
| `in` | Per-voice audio (poly) |
| `voct` | Per-voice V/oct offset added to `center`; 1.0 = +1 octave (poly) |
| `fm` | Per-voice FM sweep: ±1 sweeps ±4 octaves around `center` (poly) |
| `resonance_cv` | Per-voice bandwidth_q modulation (poly) |

**Outputs**

| Port | Description |
| --- | --- |
| `out` | Per-voice filtered audio (poly) |
