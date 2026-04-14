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

| Parameter | Type | Default | Description |
| --- | --- | --- | --- |
| `bpm` | float | `120.0` | Tempo in beats per minute (1–300) |
| `beats_per_bar` | int | `4` | Number of beats per bar (1–16) |
| `quavers_per_beat` | int | `2` | Subdivisions per beat: `2` = simple, `3` = compound (1–4) |

**Outputs**

| Port | Description |
| --- | --- |
| `bar` | Fires once per bar |
| `beat` | Fires once per beat |
| `quaver` | Fires once per quaver (`beats_per_bar × quavers_per_beat` per bar) |
| `semiquaver` | Fires once per semiquaver (twice per quaver) |
