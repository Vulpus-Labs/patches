# Detectors

> **Source of truth:** the doc comments on each module struct in
> `patches-modules/src/detectors/` define the canonical port names,
> parameter ranges, and behaviour. This page is kept in sync with those
> comments.

The detector family converts an audio signal into control events or
sustained gates by threshold-crossing the **instantaneous signed sample
value** — not envelope, magnitude, or onset. Two sub-families share a
threshold + hysteresis state machine but differ in what they emit:

- **Trigger family** (`AudioToTrigger`, `PolyAudioToTrigger`) emits
  [ADR 0047](../../../adr/0047-sub-sample-trigger-cables.md) sub-sample
  sync events on a `Trigger` cable. Intended for clock recovery from
  oscillator-rate audio, sub-octave division (a divider chain fed by an
  `AudioToTrigger` cascade), and driving hard-sync ports from an
  arbitrary audio source.
- **Gate family** (`AudioToGate`, `PolyAudioToGate`) emits sample-
  accurate sustained gates (`0.0` / `1.0`) on a normal mono / poly
  cable. Intended for sustained-state derivation from a signed-sample
  stream — the schmitt-trigger pattern.

Onset / transient detection (envelope follower → threshold) is a
different problem and not provided here.

## `AudioToTrigger` — Mono threshold-crossing detector

Fires a sub-sample sync event when the input crosses `threshold` in the
chosen direction. The event fraction is the linear-interpolated
zero-crossing position inside the current sample interval:
`frac = (threshold - prev) / (curr - prev)`, encoded on the output
`Trigger` cable per ADR 0047.

Hysteresis is a two-state machine. After a rising fire the detector
disarms; it only re-arms once the signal drops below
`threshold - hysteresis`. **Hysteresis controls eligibility, not event
location** — the fire point is `threshold`, never
`threshold - hysteresis`. The falling direction is symmetric.

Bidirectional detection tracks the rising and falling arm states
independently.

**Parameters**

| Parameter    | Type       | Default  | Description                                               |
| ------------ | ---------- | -------- | --------------------------------------------------------- |
| `threshold`  | float (dB) | `-12`    | Fire threshold (-60..0 dB)                                |
| `hysteresis` | float (dB) | `3`      | Re-arm band around threshold (0..24 dB)                   |
| `direction`  | enum       | `rising` | Edge polarity: `rising` / `falling` / `both`              |
| `cooldown`   | float (ms) | `0`      | Debounce after fire (0..1000 ms)                          |

**Inputs**

| Port | Kind | Description  |
| ---- | ---- | ------------ |
| `in` | mono | Audio source |

**Outputs**

| Port  | Kind    | Description                                                 |
| ----- | ------- | ----------------------------------------------------------- |
| `out` | trigger | Sub-sample sync event in `(0, 1]` on fire, `0.0` otherwise  |

## `PolyAudioToTrigger` — Per-voice threshold-crossing detector

Polyphonic variant. One independent `EdgeDetector` runs per voice
channel, so per-voice transitions fire on the corresponding lane of the
output `PolyTrigger` cable. Parameters apply identically across voices.

**Inputs**

| Port | Kind | Description             |
| ---- | ---- | ----------------------- |
| `in` | poly | Per-voice audio source  |

**Outputs**

| Port  | Kind         | Description                                         |
| ----- | ------------ | --------------------------------------------------- |
| `out` | poly trigger | Per-voice sub-sample sync event (independent lanes) |

Parameters identical to `AudioToTrigger`.

## `AudioToGate` — Mono Schmitt-trigger gate

Holds a sustained gate high while `signal > threshold`; closes the gate
when `signal < threshold - hysteresis`. Values inside the schmitt band
hold the previous state. Sample-accurate per
[ADR 0030](../../../adr/0030-trigger-and-gate-input-types.md) — no
sub-sample reporting.

**Parameters**

| Parameter    | Type       | Default | Description                              |
| ------------ | ---------- | ------- | ---------------------------------------- |
| `threshold`  | float (dB) | `-12`   | Open threshold (-60..0 dB)               |
| `hysteresis` | float (dB) | `3`     | Close band below threshold (0..24 dB)    |

**Inputs**

| Port | Kind | Description  |
| ---- | ---- | ------------ |
| `in` | mono | Audio source |

**Outputs**

| Port  | Kind | Description                          |
| ----- | ---- | ------------------------------------ |
| `out` | mono | Gate (`0.0` closed, `1.0` open)      |

## `PolyAudioToGate` — Per-voice Schmitt-trigger gate

Polyphonic variant. One independent `GateSchmitt` runs per voice
channel, so per-voice gate states are emitted on the lanes of the output
poly cable. Parameters apply identically across voices.

**Inputs**

| Port | Kind | Description            |
| ---- | ---- | ---------------------- |
| `in` | poly | Per-voice audio source |

**Outputs**

| Port  | Kind | Description                                          |
| ----- | ---- | ---------------------------------------------------- |
| `out` | poly | Per-voice gate (`0.0` / `1.0`, independent lanes)    |

Parameters identical to `AudioToGate`.

## No stereo variants

Neither family has a stereo variant. Both threshold-cross the signed
sample value, and there is no consistent collapse of two signed streams
into one: `(L + R) / 2` cancels antiphase content; `max(|L|, |R|)` is
rectified magnitude (contradicting the mono variants' signed semantics —
a `max(|L|, |R|)` gate would behave as an envelope-above-threshold
detector, a different operation from the mono `AudioToGate`); and
per-channel detection contradicts a single output. A patch needing a
stereo-bus gate or trigger derives it from per-channel `AudioToTrigger`
/ `AudioToGate` instances fed by `StereoSplitter`, or from a dedicated
stereo magnitude-summariser (peak / RMS) whose mono output keys an
`AudioToGate` when envelope semantics are wanted. See
[ADR 0076](../../../adr/0076-native-dynamics-and-stereo-utility-modules.md).
