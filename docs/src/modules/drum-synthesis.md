# Drum synthesis

> **Source of truth:** the doc comments on each module struct in
> `patches-modules/src/` define the canonical port names, parameter
> ranges, and behaviour. This page is kept in sync with those comments.

Patches includes a set of 808-style electronic drum synthesisers. Each module
has a single `trigger` input (rising edge) and a mono `out`. They are designed
to be driven by the tracker sequencer (see
[Tracker sequencer](tracker.md)) but work with any trigger source.

All drum modules use zero-allocation DSP kernels from `patches-dsp` and are
safe for real-time use.

---

## `Kick` — 808-style kick drum

A sine oscillator with a fast pitch sweep from a configurable start frequency
down to a settable base pitch, shaped by an exponential amplitude decay
envelope, with optional tanh saturation for grit and a transient click layer.

**Inputs**

| Port      | Kind | Description                                                    |
| --------- | ---- | -------------------------------------------------------------- |
| `trigger` | mono | Rising edge triggers                                           |
| `voct`    | mono | V/oct pitch CV; overrides `sweep` start frequency if connected |

**Outputs**

| Port  | Kind | Description |
| ----- | ---- | ----------- |
| `out` | mono | Kick signal |

**Parameters**

| Name         | Type  | Range       | Default | Description                       |
| ------------ | ----- | ----------- | ------- | --------------------------------- |
| `pitch`      | float | 20–200 Hz   | `55`    | Base pitch of the kick            |
| `sweep`      | float | 0–5000 Hz   | `2500`  | Starting frequency of pitch sweep |
| `sweep_time` | float | 0.001–0.5 s | `0.04`  | Duration of pitch sweep           |
| `decay`      | float | 0.01–2.0 s  | `0.5`   | Amplitude decay time              |
| `drive`      | float | 0.0–1.0     | `0.0`   | Saturation amount                 |
| `click`      | float | 0.0–1.0     | `0.3`   | Transient click intensity         |

---

## `Snare` — 808-style snare drum

Combines a tuned body oscillator (sine with short pitch sweep) with a
bandpass-filtered noise burst. Each path has its own decay envelope; the
`tone` parameter crossfades between them.

**Inputs**

| Port      | Kind | Description          |
| --------- | ---- | -------------------- |
| `trigger` | mono | Rising edge triggers |

**Outputs**

| Port  | Kind | Description  |
| ----- | ---- | ------------ |
| `out` | mono | Snare signal |

**Parameters**

| Name          | Type  | Range        | Default | Description                      |
| ------------- | ----- | ------------ | ------- | -------------------------------- |
| `pitch`       | float | 80–400 Hz    | `180`   | Body oscillator base pitch       |
| `tone`        | float | 0.0–1.0      | `0.5`   | Body vs noise mix (0 = all body) |
| `body_decay`  | float | 0.01–1.0 s   | `0.15`  | Body amplitude decay time        |
| `noise_decay` | float | 0.01–1.0 s   | `0.2`   | Noise amplitude decay time       |
| `noise_freq`  | float | 500–10000 Hz | `3000`  | Noise bandpass centre frequency  |
| `noise_q`     | float | 0.0–1.0      | `0.3`   | Noise bandpass resonance         |
| `snap`        | float | 0.0–1.0      | `0.5`   | Transient snap intensity         |

---

## `ClosedHiHat` — Closed hi-hat

Metallic tone from six inharmonic square oscillators mixed with
highpass-filtered white noise, shaped by a short decay envelope.

**Inputs**

| Port      | Kind | Description          |
| --------- | ---- | -------------------- |
| `trigger` | mono | Rising edge triggers |

**Outputs**

| Port  | Kind | Description       |
| ----- | ---- | ----------------- |
| `out` | mono | Closed hat signal |

**Parameters**

| Name     | Type  | Range         | Default | Description                     |
| -------- | ----- | ------------- | ------- | ------------------------------- |
| `pitch`  | float | 100–8000 Hz   | `400`   | Base frequency of metallic tone |
| `decay`  | float | 0.005–0.2 s   | `0.04`  | Amplitude decay time            |
| `tone`   | float | 0.0–1.0       | `0.5`   | Metallic vs noise mix           |
| `filter` | float | 2000–16000 Hz | `8000`  | Noise highpass cutoff           |

---

## `OpenHiHat` — Open hi-hat

Same metallic tone engine as closed hi-hat but with a longer decay range.
Includes a `choke` input so a closed hi-hat trigger can cut it short.

**Inputs**

| Port      | Kind | Description                         |
| --------- | ---- | ----------------------------------- |
| `trigger` | mono | Rising edge triggers                |
| `choke`   | mono | Rising edge chokes (cuts) the sound |

**Outputs**

| Port  | Kind | Description     |
| ----- | ---- | --------------- |
| `out` | mono | Open hat signal |

**Parameters**

| Name     | Type  | Range         | Default | Description                     |
| -------- | ----- | ------------- | ------- | ------------------------------- |
| `pitch`  | float | 100–8000 Hz   | `400`   | Base frequency of metallic tone |
| `decay`  | float | 0.05–4.0 s    | `0.5`   | Amplitude decay time            |
| `tone`   | float | 0.0–1.0       | `0.5`   | Metallic vs noise mix           |
| `filter` | float | 2000–16000 Hz | `8000`  | Noise highpass cutoff           |

---

## `Tom` — 808-style tom

Shares the kick's basic architecture (sine oscillator + pitch sweep +
amplitude decay) but with a higher pitch range, shorter sweep, and a subtle
noise layer for attack texture.

**Inputs**

| Port      | Kind | Description          |
| --------- | ---- | -------------------- |
| `trigger` | mono | Rising edge triggers |

**Outputs**

| Port  | Kind | Description |
| ----- | ---- | ----------- |
| `out` | mono | Tom signal  |

**Parameters**

| Name         | Type  | Range       | Default | Description              |
| ------------ | ----- | ----------- | ------- | ------------------------ |
| `pitch`      | float | 40–500 Hz   | `120`   | Base pitch               |
| `sweep`      | float | 0–2000 Hz   | `400`   | Pitch sweep start offset |
| `sweep_time` | float | 0.001–0.3 s | `0.03`  | Pitch sweep duration     |
| `decay`      | float | 0.05–2.0 s  | `0.3`   | Amplitude decay time     |
| `noise`      | float | 0.0–1.0     | `0.15`  | Noise layer amount       |

---

## `Cymbal` — Crash/ride cymbal

Uses the same metallic tone engine as hi-hats but with a higher frequency
range, longer decay, and a "shimmer" parameter that adds slow LFO modulation
to the partial frequencies.

**Inputs**

| Port      | Kind | Description          |
| --------- | ---- | -------------------- |
| `trigger` | mono | Rising edge triggers |

**Outputs**

| Port  | Kind | Description   |
| ----- | ---- | ------------- |
| `out` | mono | Cymbal signal |

**Parameters**

| Name      | Type  | Range         | Default | Description                        |
| --------- | ----- | ------------- | ------- | ---------------------------------- |
| `pitch`   | float | 200–10000 Hz  | `600`   | Base frequency of metallic tone    |
| `decay`   | float | 0.2–8.0 s     | `2.0`   | Amplitude decay time               |
| `tone`    | float | 0.0–1.0       | `0.5`   | Metallic vs noise mix              |
| `filter`  | float | 2000–16000 Hz | `6000`  | Noise highpass cutoff              |
| `shimmer` | float | 0.0–1.0       | `0.2`   | Partial frequency modulation depth |

---

## `Clap` — 808-style handclap

White noise passed through a bandpass filter, gated by a burst generator to
produce the initial "clappy" retriggered transient, then shaped by a longer
decay envelope for the tail.

**Inputs**

| Port      | Kind | Description          |
| --------- | ---- | -------------------- |
| `trigger` | mono | Rising edge triggers |

**Outputs**

| Port  | Kind | Description |
| ----- | ---- | ----------- |
| `out` | mono | Clap signal |

**Parameters**

| Name     | Type  | Range       | Default | Description               |
| -------- | ----- | ----------- | ------- | ------------------------- |
| `decay`  | float | 0.05–2.0 s  | `0.3`   | Tail decay time           |
| `filter` | float | 500–8000 Hz | `1200`  | Bandpass centre frequency |
| `q`      | float | 0.0–1.0     | `0.4`   | Bandpass resonance        |
| `spread` | float | 0.0–1.0     | `0.5`   | Spacing between bursts    |
| `bursts` | int   | 1–8         | `4`     | Number of noise bursts    |

---

## `Claves` — Claves

A short, bright, resonant click produced by exciting a high-Q bandpass SVF
with a single-sample impulse and shaping with a fast decay envelope.

**Inputs**

| Port      | Kind | Description          |
| --------- | ---- | -------------------- |
| `trigger` | mono | Rising edge triggers |

**Outputs**

| Port  | Kind | Description   |
| ----- | ---- | ------------- |
| `out` | mono | Claves signal |

**Parameters**

| Name    | Type  | Range       | Default | Description               |
| ------- | ----- | ----------- | ------- | ------------------------- |
| `pitch` | float | 200–5000 Hz | `2500`  | Resonant frequency        |
| `decay` | float | 0.01–0.5 s  | `0.06`  | Amplitude decay time      |
| `reson` | float | 0.3–1.0     | `0.85`  | Bandpass resonance / ring |
