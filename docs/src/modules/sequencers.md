# Sequencers & clocks

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

---

## `Seq` — Step sequencer

Advances through a list of note steps on each rising edge of the `clock`
input. Each step is a note name, a rest (`-`), or a tie (`_`).

### Pitch output convention

The `pitch` output carries a **V/oct signal referenced to C0 (≈ 16.35 Hz)**,
matching the convention used by `Osc`, `PolyOsc`, `Lfo`, and the filter
`cutoff`/`center` parameters:

| Note | V/oct value |
| --- | --- |
| C0 | `0.0` |
| C1 | `1.0` |
| C4 (middle C) | `4.0` |
| A4 (concert A) | `4.75` |
| C0 + 1 semitone (C#0) | `≈ 0.083` |

This is the same scale that results from writing a note literal directly as a
parameter value (e.g. `frequency: C4` in a module declaration).

### Step syntax

| Step string | Meaning |
| --- | --- |
| `C4`, `A#3`, `Bb2` | Note: sets pitch, gate=1, trigger=1 for one sample |
| `-` | Rest: gate=0, trigger=0; pitch holds its previous value |
| `_` | Tie: gate=1, trigger=0; pitch holds its previous value |

Note names are case-sensitive. Sharps use `#`; flats use `b`. Octave digits
`0`–`9` are supported.

**Parameters**

| Parameter | Type | Default | Description |
| --- | --- | --- | --- |
| `steps` | string array | `[]` | Ordered list of step strings |

**Inputs**

| Port | Description |
| --- | --- |
| `clock` | Rising edge (≥ 0.5, from below) advances to the next step |
| `start` | Rising edge resumes playback |
| `stop` | Rising edge pauses playback (gate and trigger go to 0) |
| `reset` | Rising edge returns to step 0 without advancing |

**Outputs**

| Port | Description |
| --- | --- |
| `pitch` | Current pitch as V/oct above C0 |
| `trigger` | `1.0` for one sample on each note step advance, otherwise `0.0` |
| `gate` | `1.0` while a note or tie step is current and playback is active |

### Hot-reload behaviour

When the `steps` list is updated by a hot-reload, the current step index is
preserved so playback continues from the same position without an audible
jump. If the new list is shorter, the sequencer treats the out-of-range
position as a rest until the next clock edge wraps the index back in bounds.
