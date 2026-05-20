# Tracker sequencer

> **Source of truth:** the doc comments on each module struct in
> `patches-modules/src/` define the canonical port names, parameter
> ranges, and behaviour. This page is kept in sync with those comments.

The tracker sequencer is a two-module system for song-driven step sequencing.
A `MasterSequencer` reads a named song from the patch's tracker data and
drives one or more `PatternPlayer` modules via poly clock buses. Pattern and
song data are defined in the `.patches` file using `pattern` and `song`
blocks (see [DSL reference — Patterns & songs](../dsl-reference.md#patterns)).

## How it fits together

```text
┌─────────────────────┐
│  MasterSequencer    │
│  song: my_song      │──clock[bass]──▶ PatternPlayer (channels: [note, vel])
│  bpm: 120           │──clock[lead]──▶ PatternPlayer (channels: [note, vel])
│  rows_per_beat: 4   │──clock[drums]─▶ PatternPlayer (channels: [hit])
└─────────────────────┘
```

The MasterSequencer walks through the song order, emitting timing and
pattern-selection data on each clock bus. Each PatternPlayer decodes that
data, looks up the relevant pattern, and outputs cv/trigger/gate signals per
channel.

---

## `MasterSequencer` — Song-driven transport

Drives song playback with transport controls, swing, and a poly clock bus per
song channel.

**Inputs**

| Port     | Kind | Description                                 |
| -------- | ---- | ------------------------------------------- |
| `start`  | mono | Rising edge resets and begins playback      |
| `stop`   | mono | Rising edge halts and resets playback       |
| `pause`  | mono | Rising edge halts playback in place         |
| `resume` | mono | Rising edge continues from current position |

**Outputs**

| Port       | Kind | Description                                          |
| ---------- | ---- | ---------------------------------------------------- |
| `clock[i]` | poly | Clock bus per channel (i in 0..N−1, N = `channels`)  |

The clock bus carries four poly voices:

| Voice | Signal              | Description                                |
| ----- | ------------------- | ------------------------------------------ |
| 0     | pattern reset       | 1.0 on first tick of a new pattern         |
| 1     | pattern bank index  | float-encoded integer (−1 = stop sentinel) |
| 2     | tick trigger        | 1.0 on each step                           |
| 3     | tick duration       | seconds per tick                           |

**Parameters**

| Name            | Type      | Range          | Default | Description                                                     |
| --------------- | --------- | -------------- | ------- | --------------------------------------------------------------- |
| `bpm`           | float     | 1.0–999.0      | `120.0` | Tempo in beats per minute                                       |
| `rows_per_beat` | int       | 1–64           | `4`     | Steps per beat                                                  |
| `song`          | song_name | —              | none    | Name of the song to play                                        |
| `loop`          | bool      | —              | `true`  | Loop at end of song                                             |
| `autostart`     | bool      | —              | `true`  | Begin playback on activation                                    |
| `swing`         | float     | 0.0–1.0        | `0.5`   | Swing ratio for alternating steps                               |
| `sync`          | enum      | auto/free/host | `auto`  | Clock source: `auto` picks `host` when hosted, otherwise `free` |

**Shape arguments**

The `channels` shape argument defines the song channels. Use named aliases
to match the song's lane names:

```patches
module seq: MasterSequencer(channels: [bass, lead, drums]) {
    song: my_song, bpm: 140, rows_per_beat: 4
}
```

---

## `PatternPlayer` — Pattern step sequencer

A generic multi-channel step sequencer that reads a poly clock bus, steps
through pattern data, and outputs cv1/cv2/trigger/gate signals per channel.
The PatternPlayer does not know whether its channels are notes, drums, or
automation — all channels produce the same four output types.

**Inputs**

| Port    | Kind | Description                     |
| ------- | ---- | ------------------------------- |
| `clock` | poly | Clock bus from MasterSequencer  |

**Outputs**

| Port          | Kind | Description                                          |
| ------------- | ---- | ---------------------------------------------------- |
| `cv1[i]`      | mono | Control voltage 1 per channel (i in 0..N−1)          |
| `cv2[i]`      | mono | Control voltage 2 per channel (i in 0..N−1)          |
| `trigger[i]`  | mono | Trigger per channel (i in 0..N−1)                    |
| `gate[i]`     | mono | Gate per channel (i in 0..N−1)                       |

**Shape arguments**

The `channels` shape argument defines the pattern channels. Use named
aliases to match the pattern's channel names:

```patches
module pp: PatternPlayer(channels: [note, vel])
```

### Output conventions

For note channels, `cv1` carries the V/oct pitch (same convention as `Osc`)
and `trigger`/`gate` signal note-on events. For drum/trigger
channels, `trigger` fires on each hit and `cv2` carries the velocity value.

### Step events (ADR 0077 unified grammar)

The PatternPlayer consumes the row-build pass's resolved `StepEffect`
per cell — a single typed dispatch covering rests, triggered notes,
rolls, slides, step-cv changes, and continuation cells. See the
[DSL reference — Step events](../dsl-reference.md#step-events) for the
full cell taxonomy, close rule, and worked-example timelines. The
player-side behaviour is:

- **Triggered cell** (`value`, `value*N`, `value:cv2`, `value>value`,
  `value>`): snap cv1/cv2 at the start of the tick and fire `trigger`.
  `*N` opens a roll; `value>value` opens-and-closes a one-tick slide;
  `value>` opens a multi-tick slide whose close target is resolved
  when the close cell arrives.
- **Step-cv** (`/value`): snap cv1 (and `:cv2` if present) at the tick
  boundary; no trigger. Gate stays high.
- **Slide flow** (`>_`, `_` absorbed by a slide): no trigger; cv ramps
  per the slide's per-channel schedule. Gate stays high.
- **Slide close-in-tick** (`>value`): cv1 (and `:cv2` if present) ramp
  to the close value across this tick; no trigger.
- **Tie sustain** (`_` with no open modifier): gate stays high; cv
  held.
- **Tie-spread roll** (`_` absorbed by a `*N` anchor): the N
  sub-triggers spread evenly across the anchor tick plus the absorbed
  run. Per-tick sub-event placement respects swung tick durations —
  each sub-event's sample offset is computed against the *current*
  tick's duration, not the anchor's, so swing within a span sits
  precisely on the swung beats.
- **Rest** (`.`): gate drops; channel state resets (any open slide is
  closed at its current cv).

```patches
# tie-spread roll across two ticks then a fresh hit
hit: x*3 _ x .

# slide opens at tick 1, ramps through ticks 1+2, lands on G4 at the
# boundary, holds tick 3
note: E4> _ /G4

# slide opens, ramps continuously across three ticks, finishes inside
# tick 3
note: E4> _ >G4

# step cv without retrigger
note: E4 /G4 _
```

---

## Wiring examples

### Drum machine

One PatternPlayer per drum voice, each with a single `hit` channel:

```patches
module kick_pp: PatternPlayer(channels: [hit])
seq.clock[kick] -> kick_pp.clock

module kick_drum: Kick { pitch: 50, decay: 0.4 }
kick_pp.trigger[hit] -> kick_drum.trigger
kick_pp.cv2[hit]     -> kick_vca.velocity
```

### Melodic voice

A PatternPlayer with `note` and `vel` channels driving a synth voice:

```patches
module bass_pp: PatternPlayer(channels: [note, vel])
seq.clock[bass] -> bass_pp.clock

bass_pp.cv1[note]     -> voice.voct
bass_pp.gate[note]    -> voice.gate
bass_pp.trigger[note] -> voice.trigger
bass_pp.cv1[vel]      -> voice.velocity
```
