# Envelopes

> **Source of truth:** the doc comments on each module struct in
> `patches-modules/src/` define the canonical port names, parameter
> ranges, and behaviour. This page is kept in sync with those comments.

## `Adsr` — Multi-channel DADSR envelope with built-in VCA

Multi-channel Attack-Decay-Sustain-Release envelope generator with an
optional pre-attack **D**elay phase and built-in VCA pass-through. One
module emits N independent envelopes (one per channel) driven by a
single shared `trigger` and `gate`.

With `channels = 1` (the default) it behaves as a conventional
single-channel ADSR — `env.out`, `env.vca_in`, `env.vca_out` address
channel 0 implicitly. Use `Adsr([amp, filt, vib])` or `Adsr(3)` to get
multiple envelopes per trigger/gate.

Each channel has its own delay/attack/decay/sustain/release/shape, and
an optional VCA stage: if `vca_in[c]` is connected, `vca_out[c]` emits
`vca_in[c] * env[c]`. Useful for tying amplitude, filter, and
modulation envelopes to the same note event without wiring N separate
modules. A rising `trigger` enters the per-channel Delay phase (output
held at 0 for `delay[c]` seconds) before Attack begins. Releasing the
gate during Delay returns the channel to Idle without emitting any
envelope.

**Inputs**

| Port        | Kind    | Description                                                                |
| ----------- | ------- | -------------------------------------------------------------------------- |
| `trigger`   | trigger | Shared one-sample pulse starts the envelope on every channel    |
| `gate`      | mono    | Shared: held high to sustain; release to enter Release on every channel    |
| `vca_in[i]` | mono    | Optional audio/CV input multiplied by the channel's envelope (i in 0..N-1) |

**Outputs**

| Port         | Kind | Description                                    |
| ------------ | ---- | ---------------------------------------------- |
| `out[i]`     | mono | Envelope level for channel i in [0.0, 1.0]     |
| `vca_out[i]` | mono | `vca_in[i] * out[i]` — pre-multiplied audio/CV |

**Parameters**

| Name         | Type  | Range               | Default  | Description                                         |
| ------------ | ----- | ------------------- | -------- | --------------------------------------------------- |
| `delay[i]`   | float | 0.0 – 10.0          | `0.0`    | Pre-attack hold time in seconds (output stays at 0) |
| `attack[i]`  | float | 0.001 – 10.0        | `0.01`   | Attack time in seconds                              |
| `decay[i]`   | float | 0.001 – 10.0        | `0.1`    | Decay time in seconds                               |
| `sustain[i]` | float | 0.0 – 1.0           | `0.7`    | Sustain level                                       |
| `release[i]` | float | 0.001 – 10.0        | `0.3`    | Release time in seconds                             |
| `shape[i]`   | enum  | linear, exponential | `linear` | Segment shape: linear ramp or analog-style RC curve |

---

## `PolyAdsr` — Per-voice DADSR

Same envelope shape as `Adsr` (delay/attack/decay/sustain/release/shape)
but each voice has independent state, driven by per-voice
`trigger` / `gate` poly cables. Optional per-voice VCA via `vca_in`.

**Inputs**

| Port      | Kind | Description                                            |
| --------- | ---- | ------------------------------------------------------ |
| `trigger` | poly | Per-voice sub-sample trigger                           |
| `gate`    | poly | Per-voice gate                                         |
| `vca_in`  | poly | Optional per-voice audio/CV multiplied by the envelope |

**Outputs**

| Port      | Kind | Description                            |
| --------- | ---- | -------------------------------------- |
| `out`     | poly | Per-voice envelope level in [0.0, 1.0] |
| `vca_out` | poly | `vca_in * out` per voice               |

**Parameters** — same names and ranges as `Adsr`. Parameters apply
identically to all voices.
