# Sequencers & clocks

> **Source of truth:** the doc comments on each module struct in
> `patches-modules/src/` define the canonical port names, parameter
> ranges, and behaviour. This page is kept in sync with those comments.

## `Clock` — Tempo-locked trigger generator

Generates bar, beat, quaver, and semiquaver trigger pulses from a configurable
BPM and time signature. All outputs are derived from a single beat-phase
accumulator, so they are always phase-locked to each other.

Each output fires a one-sample pulse (`1.0`) at the relevant boundary and is
`0.0` on all other samples.

**Parameters**

| Parameter          | Type  | Default | Description                                               |
| ------------------ | ----- | ------- | --------------------------------------------------------- |
| `bpm`              | float | `120.0` | Tempo in beats per minute (1–300)                         |
| `beats_per_bar`    | int   | `4`     | Number of beats per bar (1–16)                            |
| `quavers_per_beat` | int   | `2`     | Subdivisions per beat: `2` = simple, `3` = compound (1–4) |

**Outputs**

| Port         | Description                                                        |
| ------------ | ------------------------------------------------------------------ |
| `bar`        | Fires once per bar                                                 |
| `beat`       | Fires once per beat                                                |
| `quaver`     | Fires once per quaver (`beats_per_bar × quavers_per_beat` per bar) |
| `semiquaver` | Fires once per semiquaver (twice per quaver)                       |

---

## `HostTransport` — DAW transport readback

Exposes the host DAW's transport state as CV signals. In standalone mode
the player synthesises a static transport (tempo from `--bpm`, no
playhead motion).

**Outputs**

| Port           | Kind    | Description                                                 |
| -------------- | ------- | ----------------------------------------------------------- |
| `playing`      | mono    | `1.0` while the host is playing, otherwise `0.0`            |
| `tempo`        | mono    | Current tempo in BPM                                        |
| `beat`         | mono    | Position within the bar as fractional beats                 |
| `bar`          | mono    | Bar index (integer-valued, increments at each bar boundary) |
| `beat_trigger` | trigger | Fires once per beat boundary                                |
| `bar_trigger`  | trigger | Fires once per bar boundary                                 |
| `tsig_num`     | mono    | Time signature numerator                                    |
| `tsig_denom`   | mono    | Time signature denominator                                  |

---

## `TempoSync` — BPM + subdivision → milliseconds

Converts a tempo (in BPM) and a fixed musical subdivision into a duration
in milliseconds. Drives `delay_ms`-style inputs on `Delay`, `MsTicker`,
etc. for tempo-locked timing.

**Parameters**

| Name          | Type | Default | Description                                                                                                 |
| ------------- | ---- | ------- | ----------------------------------------------------------------------------------------------------------- |
| `subdivision` | enum | `1/4`   | Subdivision: `1/1`, `1/2`, `1/4`, `1/8`, `1/16`, `1/32`, plus dotted (`1/4d`) and triplet (`1/4t`) variants |

**Inputs**

| Port  | Kind | Description  |
| ----- | ---- | ------------ |
| `bpm` | mono | Tempo in BPM |

**Outputs**

| Port | Kind | Description                                 |
| ---- | ---- | ------------------------------------------- |
| `ms` | mono | Duration of one subdivision in milliseconds |

---

## `MsTicker` — Millisecond-paced trigger

Fires a trigger every `ms` milliseconds with a corresponding gate.

**Inputs**

| Port    | Kind    | Description                            |
| ------- | ------- | -------------------------------------- |
| `ms`    | mono    | Inter-trigger interval in milliseconds |
| `reset` | trigger | Resets the internal phase to zero      |

**Outputs**

| Port      | Kind    | Description                                                  |
| --------- | ------- | ------------------------------------------------------------ |
| `trigger` | trigger | Sub-sample trigger at each tick boundary          |
| `gate`    | mono    | High between trigger and next reset; useful as a duty signal |

---

## `TriggerToSync` / `SyncToTrigger`

A pair of converters between the two trigger encodings: legacy mono level
pulses (≥ 0.5 high) and the sub-sample sync cable. Use when
bridging modules that expect different encodings.

**`TriggerToSync`** — mono `in` → trigger `out`.
**`SyncToTrigger`** — trigger `in` → mono `out` (rising-edge pulse).
