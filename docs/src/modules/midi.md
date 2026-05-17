# MIDI input

> **Source of truth:** the doc comments on each module struct in
> `patches-modules/src/` define the canonical port names, parameter
> ranges, and behaviour. This page is kept in sync with those comments.

MIDI events arrive on the engine-level `GLOBAL_MIDI` backplane (carrying up
to 5 packed events per sample). Modules that need MIDI either read the
backplane directly (`MidiToCv`, `PolyMidiToCv`, `MidiCC`) or sit downstream
of a `MidiIn` module which exposes the backplane as a `midi` cable so
other MIDI-processing modules can be chained.

## `MidiIn` — Backplane → MIDI cable

Exposes the engine's `GLOBAL_MIDI` backplane as a `midi` output cable so
that `MidiSplit`, `MidiTranspose`, `MidiArp`, `MidiDelay`, etc. can be
chained from it.

**Outputs**

| Port   | Kind | Description                       |
| ------ | ---- | --------------------------------- |
| `midi` | midi | MIDI events from the system input |

## `MidiToCv` — Mono MIDI note input

Monophonic: only one note active at a time (last-note priority). Pressing a
new key updates pitch immediately; releasing the top key falls back to the
previously held key without retriggering. Up to 16 simultaneously held keys
are tracked.

### V/oct convention

`voct` is referenced to MIDI note 0 (C0 ≈ 16.35 Hz) at 0 V, with 1/12 V per
semitone:

| MIDI note | Note           | `voct` |
| --------- | -------------- | ------ |
| 0         | C0             | `0.0`  |
| 12        | C1             | `1.0`  |
| 60        | C4 (middle C)  | `5.0`  |
| 69        | A4 (concert A) | `5.75` |

**Outputs**

| Port       | Description                                                                                       |
| ---------- | ------------------------------------------------------------------------------------------------- |
| `voct`     | Pitch in V/oct above C0 (MIDI note 0 = 0.0, 1/12 V per semitone)                                  |
| `trigger`  | Sub-sample trigger on each note-on                                                     |
| `gate`     | `1.0` while any note is held or sustain pedal (CC 64) is active                                   |
| `mod`      | CC 1 (mod wheel) normalised to [0.0, 1.0]                                                         |
| `pitch`    | Pitchbend normalised to [−1.0, 1.0] (centre = 0.0)                                                |
| `velocity` | Most-recent note-on velocity normalised to [0.0, 1.0]                                             |
| `slur`     | `1.0` while a held key falls back to a still-held lower key without retriggering, otherwise `0.0` |

**Parameters**

| Name     | Type | Default | Description                                                                                                                 |
| -------- | ---- | ------- | --------------------------------------------------------------------------------------------------------------------------- |
| `legato` | bool | `false` | When `true`, holding through a note change suppresses the per-note `trigger` (gate stays high; only the first key triggers) |

---

## `PolyMidiToCv` — Polyphonic MIDI input

Distributes incoming notes across up to 16 voices with LIFO voice stealing.
When all voices are occupied the most-recently-allocated voice is stolen.

**Outputs**

| Port       | Kind | Description                                            |
| ---------- | ---- | ------------------------------------------------------ |
| `voct`     | Poly | Per-voice pitch in V/oct above C0                      |
| `trigger`  | Poly | Per-voice sub-sample trigger on note-on     |
| `gate`     | Poly | `1.0` while the note for that voice is physically held |
| `velocity` | Poly | Per-voice note-on velocity normalised to [0.0, 1.0]    |
| `mod`      | Mono | CC 1 (mod wheel) normalised to [0.0, 1.0]              |
| `pitch`    | Mono | Pitchbend normalised to [−1.0, 1.0]                    |

---

## `MidiCC` — Control-change reader

Reads a configured MIDI CC number from the backplane and emits its
normalised value as a mono signal.

**Parameters**

| Name | Type        | Default | Description            |
| ---- | ----------- | ------- | ---------------------- |
| `cc` | int (0–127) | `1`     | MIDI CC number to read |

**Outputs**

| Port  | Kind | Description                                             |
| ----- | ---- | ------------------------------------------------------- |
| `out` | mono | Most-recent CC value for `cc`, normalised to [0.0, 1.0] |

---

## `MidiArp` — Arpeggiator

Arpeggiates held notes, clocked by an external trigger (e.g. `Clock`).
Refer to the module doc comment in `patches-modules/src/midi_arp.rs` for
the current parameter set and port list.

## `MidiDelay` — MIDI note delay

Delays incoming MIDI events by a configurable number of samples or beats,
with optional feedback for repeating echoes. See
`patches-modules/src/midi_delay.rs` for ports and parameters.

## `MidiSplit` — MIDI splitter

Routes incoming MIDI notes to one of several outputs based on channel,
velocity, or pitch range. See `patches-modules/src/midi_split.rs`.

## `MidiTranspose` — MIDI pitch transposer

Transposes note-on / note-off events by a configurable number of
semitones. See `patches-modules/src/midi_transpose.rs`.

## `MidiDrumset` — MIDI note-to-drum mapper

Maps incoming MIDI notes to trigger outputs for drum-voice modules
(`Kick`, `Snare`, etc). See `patches-modules/src/midi_drumset.rs`.
