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
| `trigger`   | trigger | Shared one-sample pulse starts the envelope on every channel               |
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

## `Env` — Multi-stage breakpoint envelope with key-follow

Where `Adsr` has a fixed attack/decay/sustain/release shape, `Env` runs
an arbitrary number of `(time, level, curve)` stages with a designated
sustain stage — enough to express D50-style contours ADSR cannot (attack
spike → dip → secondary swell → sustain). The stage count is the
module's *channels* axis: `Env(5)` is a five-stage envelope. One module
is one envelope (mono); use multiple instances for multiple envelopes.

Stages `0..sustain_stage` form the pre-sustain contour; the envelope
holds at `level[sustain_stage]` while the gate is high, then runs the
remaining stages `sustain_stage+1..N` as the release tail on gate-off.
Designate a final stage with `level = 0` for a release to silence. A
re-trigger restarts from the current level (no click), and a gate-off
during the pre-sustain contour jumps straight into the release tail.

**Key-follow** shortens stage times with pitch, as real resonators decay
faster up the keyboard: `time_scale = 2^(-keyfollow * (voct - ref_key))`,
so `keyfollow = 1.0` halves all stage times one octave above `ref_key`.
It is applied per tick, so a bending `voct` re-scales the contour live.

**Velocity** scales stage *levels*: the effective `velocity` (1.0 if the
input is unconnected) is mapped to a level multiplier
`1 - vel_depth * (1 - velocity)` and latched at the trigger, so an
in-flight envelope keeps a stable scaling.

Route `out` to an oscillator `voct` / `phase_mod` or a filter cutoff for
the D50 attack pitch-blip or filter sweep — that's a patch idiom, not a
port.

**Inputs**

| Port       | Kind    | Description                                                     |
| ---------- | ------- | --------------------------------------------------------------- |
| `trigger`  | trigger | One-sample pulse starts the envelope                            |
| `gate`     | mono    | Held high to sustain; release to run the release tail           |
| `voct`     | mono    | 1V/oct pitch driving key-follow time-scaling (0 if unconnected) |
| `velocity` | mono    | Velocity in [0, 1] scaling stage levels (1.0 if unconnected)    |
| `vca_in`   | mono    | Optional audio/CV input multiplied by the envelope              |

**Outputs**

| Port      | Kind | Description                     |
| --------- | ---- | ------------------------------- |
| `out`     | mono | Envelope level in [0.0, 1.0]    |
| `vca_out` | mono | `vca_in * out` — pre-multiplied |

**Parameters**

| Name            | Type             | Range               | Default  | Description                                                |
| --------------- | ---------------- | ------------------- | -------- | ---------------------------------------------------------- |
| `time[i]`       | float            | 0.0 – 10.0          | `0.1`    | Stage i duration in seconds (i in 0..N−1, N = stage count) |
| `level[i]`      | float            | 0.0 – 1.0           | `1.0`    | Stage i target level                                       |
| `curve[i]`      | enum             | linear, exponential | `linear` | Stage i segment shape                                      |
| `sustain_stage` | int (structural) | 0 – 7               | `0`      | Index of the held stage; later stages are the release tail |
| `keyfollow`     | float            | 0.0 – 1.0           | `0.0`    | Pitch time-scaling depth (1.0 halves times one octave up)  |
| `ref_key`       | float            | -5.0 – 5.0          | `0.0`    | Reference pitch (V/oct) at which `time_scale = 1`          |
| `vel_depth`     | float            | 0.0 – 1.0           | `0.0`    | How much velocity attenuates stage levels                  |

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

---

## `PolyEnv` — Per-voice multi-stage envelope

The 16-voice form of `Env`: same multi-stage contour, key-follow, and
velocity scaling, but each voice holds independent state driven by
per-voice `trigger` / `gate` / `voct` / `velocity` poly cables. All
voices share one stage configuration. The stage count is the channels
axis (`PolyEnv(5)` = five stages); `sustain_stage` and the release-tail
semantics are identical to `Env`.

Key-follow and velocity are computed per voice:
`time_scale = 2^(-keyfollow * (voct[v] - ref_key))` and the level
multiplier `1 - vel_depth * (1 - velocity[v])`, latched at each voice's
trigger. Unconnected `voct` reads as 0 and unconnected `velocity` as 1.0.

**Inputs**

| Port       | Kind         | Description                                            |
| ---------- | ------------ | ------------------------------------------------------ |
| `trigger`  | poly_trigger | Per-voice rising edge starts the envelope              |
| `gate`     | poly         | Per-voice gate; release to run the release tail        |
| `voct`     | poly         | Per-voice 1V/oct pitch driving key-follow              |
| `velocity` | poly         | Per-voice velocity in [0, 1] scaling stage levels      |
| `vca_in`   | poly         | Optional per-voice audio/CV multiplied by the envelope |

**Outputs**

| Port      | Kind | Description                            |
| --------- | ---- | -------------------------------------- |
| `out`     | poly | Per-voice envelope level in [0.0, 1.0] |
| `vca_out` | poly | `vca_in * out` per voice               |

**Parameters** — same names, ranges, and defaults as `Env`
(`time[i]`, `level[i]`, `curve[i]`, `sustain_stage`, `keyfollow`,
`ref_key`, `vel_depth`). Parameters apply identically to all voices.
