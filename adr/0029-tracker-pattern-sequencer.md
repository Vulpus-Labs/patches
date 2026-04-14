# ADR 0029 — Tracker-style pattern sequencer

**Date:** 2026-04-11
**Status:** proposed
**Superseded in part by:** [ADR 0035](0035-song-sections-play-composition.md)
— the pipe-table song-body syntax has been replaced by lane-declared song
headers with `section` and `play` composition. The `pattern`,
`MasterSequencer`, and `PatternPlayer` portions of this ADR are unchanged.

---

## Context

Patches currently relies on `PolyMidiIn` for note input — either live MIDI
or external sequencers. There is no built-in way to define note sequences,
drum patterns, or automation lanes in the DSL itself. A tracker-style
sequencer would let users compose rhythmic and melodic material directly in
`.patches` files, with hot-reload for live iteration.

Classic trackers (ProTracker, Renoise, etc.) organise music as:

1. **Patterns** — fixed-length grids of step data across parallel channels.
2. **A song order** — a sequence of pattern assignments per channel.
3. **A master clock** — drives playback at a given tempo.

This ADR describes how these concepts map onto the Patches architecture.

---

## Decision

### New DSL constructs: `pattern` and `song` blocks

Two new top-level block types are added to the grammar, alongside the
existing `template` and `patch` blocks.

#### Pattern blocks

A `pattern` block defines a named, multi-channel grid of step data:

```patches
pattern verse_drums {
    kick:   x . . . x . . . x . . . x . . .
    snare:  . . x . . . x . . . x . . . x .
    open:   x . . . . . . . . . . . . . . .
    closed: . x x x . x x x . x x x . x x x
}

pattern melody {
    note:   C4 Eb4 .  C4 Eb4 F4 .  Eb4
    vel:    1.0 0.8 .  0.8 1.0 0.8 .  0.8
    cutoff: slide(4, 0.2, 0.8) slide(4, 0.8, 0.2)
}
```

Channel names are declared by the row labels inside the block. The step
count is inferred from the longest channel row (shorter rows are padded
with rests).

A trailing `|` on a channel row continues it on the next line, allowing
longer patterns to be written readably:

```patches
pattern long_bass {
    note: C2 . . ~ E2 . G2 . C2 . . ~ A1 . . . |
          C2 . E2 ~ G2 . . . A1 . . ~ G1 . . .
}
```

#### Step notation

Every step produces the same four values regardless of channel purpose:
`cv1` (float), `cv2` (float), `trigger` (bool), `gate` (bool). The
notation provides sugar for common cases:

| Syntax | cv1 | cv2 | trigger | gate |
|--------|-----|-----|---------|------|
| `C4` | 4.0 (v/oct) | prev | true | true |
| `C4:0.8` | 4.0 | 0.8 | true | true |
| `x` | 0.0 | prev | true | true |
| `x:0.7` | 0.0 | 0.7 | true | true |
| `0.5` | 0.5 | prev | true | true |
| `0.5:0.3` | 0.5 | 0.3 | true | true |
| `.` | 0.0 | 0.0 | false | false |
| `~` | prev cv1 | prev cv2 | false | true |

`x` is equivalent to `C0` (v/oct 0.0). Note literals, `x`, and float
literals are all ways of writing cv1; the colon separator provides cv2.
When cv2 is omitted, it carries over from the previous step.

Unit suffixes (`Hz`, `kHz`, `dB`, etc.) are resolved to float values at
parse time, using the same unit conversion as existing DSL parameters.

#### Slides

A slide specifies start and end values for interpolation over the tick
duration:

```
C4>E4         cv1 slides from C4 to E4, cv2 carries over
C4>E4:0.5>0.8  cv1 slides C4→E4, cv2 slides 0.5→0.8
C4:0.5>0.8    cv1 holds, cv2 slides 0.5→0.8
```

Each step is self-contained — the start and end values are both specified
in the step, so no lookahead into adjacent steps is required.

The `slide()` generator is expand-time sugar that produces a sequence of
slide steps over cv1 only (cv2 carries over from the preceding step):

```
slide(4, 0.0, 1.0)  →  0.0>0.25 0.25>0.5 0.5>0.75 0.75>1.0
```

#### Repeats

A `*n` suffix subdivides the tick into `n` evenly-spaced triggers:

```
x*3       three triggers within the tick (triplet roll)
C4*2:0.8  two triggers, both with cv2=0.8
```

The PatternPlayer uses the tick duration (from the clock bus) to calculate
sub-trigger timing.

#### Song blocks

A `song` block defines a named arrangement — which patterns play in which
order across song-level channels. Multiple songs can be defined in a
single file:

```patches
song my_song {
    | drums       | lead     | bass   |
    | verse_drums | melody_a | bass_a |
    | verse_drums | melody_a | bass_a |
    | fill_drums  | melody_b | bass_b |
    | verse_drums | melody_a | bass_a |
}

song alt_song {
    | drums       | lead     |
    | fill_drums  | melody_b |
    | verse_drums | melody_a |
}
```

The first row is the header, declaring the channel names. Subsequent rows
are the arrangement — each row is one pattern-length block. `_` denotes
silence on a channel for that row.

Song channels are distinct from pattern channels: a song channel
corresponds to one PatternPlayer module instance, while pattern channels
are the parallel data lanes within that player.

#### Song loop points

A `@loop` annotation on a row marks the loop point — when `loop` is
`true` on the MasterSequencer, playback jumps back to this row after
reaching the end:

```patches
song my_song {
    | drums       | lead     |
    | verse_drums | melody_a |
    | verse_drums | melody_a |  @loop
    | fill_drums  | melody_b |
    | verse_drums | melody_a |
}
```

Here, the first two rows play once (intro), then rows 2–4 loop
indefinitely. If no `@loop` annotation is present, the entire song loops
from the beginning.

Loop points are part of the `Song` data structure, not expanded at parse
time. The MasterSequencer checks its current row against the song length
and jumps to the loop point row index. This keeps the data compact and
allows the loop point to be changed via hot-reload without duplicating
rows.

### Modules

#### MasterSequencer

Drives playback. Receives the song order and outputs a poly clock bus per
song channel.

```
Parameters: bpm (float), rows_per_beat (int), song (string),
            loop (bool), autostart (bool), swing (float)
Inputs:     start (mono), stop (mono), pause (mono), resume (mono)
Outputs:    clock (poly_out_multi, one per song channel)
```

Each clock output is a poly signal carrying a structured control bus:

| Voice | Signal | Description |
|-------|--------|-------------|
| 0 | pattern reset | 1.0 on first tick of a new pattern |
| 1 | pattern bank index | float-encoded integer |
| 2 | tick trigger | 1.0 on each step |
| 3 | tick duration | seconds (e.g. 0.125 for 120 bpm, 4 rows/beat) |

Encoding timing as an explicit tick duration on the bus means
PatternPlayers need no knowledge of BPM or swing — they just see
per-tick durations.

#### Swing

The `swing` parameter (float, 0.0–1.0, default 0.5) controls the timing
ratio of alternating steps within each beat. A pair of consecutive steps
has a combined duration of `2 * base_tick`. The swing value determines
how that time is divided:

- Even step duration: `2 * base_tick * swing`
- Odd step duration: `2 * base_tick * (1.0 - swing)`

At `0.5`, all steps are equal (straight time). At `0.67`, even steps are
twice as long as odd steps (classic swing feel). At `0.33`, the pattern
reverses (short-long).

The MasterSequencer implements swing entirely by varying voice 3 (tick
duration) on the clock bus. PatternPlayers are unaffected — slides and
repeats scale correctly because they interpolate over the tick duration
they receive.

The MasterSequencer declares its channels explicitly, like any other
multi-channel module. It selects which song to play via a `song`
parameter:

```patches
module seq: MasterSequencer(channels: [drums, bass, lead]) {
    song: my_song,
    bpm: 120,
    rows_per_beat: 4,
    swing: 0.5,
    loop: true,
    autostart: true
}
```

The interpreter validates that the song's column headers match the
module's declared channels. The song data is delivered via
`Arc<TrackerData>` (see below); the MasterSequencer looks up its song by
name and reads the order table from there.

#### Transport controls

The MasterSequencer has four trigger-receiving mono inputs for transport:

- **start** — reset to the beginning and begin playback.
- **stop** — halt playback and reset position to the beginning.
- **pause** — halt playback, retaining the current position.
- **resume** — continue playback from the current position.

If `autostart` is `true` (the default), playback begins immediately on
plan activation. If `false`, the sequencer waits for a rising edge on the
`start` input.

These inputs can be driven by any trigger source. For MIDI transport
control, a `MidiTransport` module (or the existing `MidiMix`) can convert
MIDI System Real-Time messages to triggers:

- MIDI Start (0xFA) → `start`
- MIDI Stop (0xFC) → `stop`
- MIDI Continue (0xFB) → `resume`

```patches
module midi: MidiTransport
module seq: MasterSequencer(channels: [drums, bass]) {
    song: my_song, bpm: 120, rows_per_beat: 4, autostart: false
}

midi.start  -> seq.start
midi.stop   -> seq.stop
midi.resume -> seq.resume
```

#### End-of-song behaviour

When the MasterSequencer reaches the end of the song order:

- If `loop` is `true`, playback continues from the loop point (see
  below).
- If `loop` is `false`, the sequencer stops emitting tick triggers. The
  PatternPlayers receive no further ticks, and their gate outputs clear
  naturally — a step with no subsequent trigger produces gate=false,
  effectively silencing all channels.

#### PatternPlayer gate clearing

A PatternPlayer's gate outputs reflect the current step's gate value. When
no tick trigger arrives (because the sequencer has stopped or paused), the
PatternPlayer holds its last state. To ensure clean silence at song end,
the MasterSequencer emits one final tick with a special "stop" indication
(pattern bank index of -1 / sentinel value), which causes PatternPlayers
to clear all gates and cease output.

#### PatternPlayer

A generic multi-channel step sequencer. Reads a poly clock bus, steps
through pattern data, and outputs control signals.

```
Shape:   channels (int, with optional aliases)
Inputs:  clock (poly)
Outputs: cv1 (mono_out_multi), cv2 (mono_out_multi),
         trigger (mono_out_multi), gate (mono_out_multi)
```

All four output types are created for every channel. Unused outputs are
skipped at runtime via the connected-port check — a drum channel where
only `trigger` is wired has zero overhead for `cv1`, `cv2`, and `gate`.

The PatternPlayer does not declare which patterns it can play. Instead, it
receives the complete `TrackerData` at plan activation (see below) and
plays whichever pattern the clock bus selects via voice 1.

### Tracker data: shared via `Arc<TrackerData>`

All `pattern` and `song` blocks in a `.patches` file are collected into a
single `TrackerData` at interpret/plan time:

```rust
struct TrackerData {
    patterns: PatternBank,
    songs: SongBank,
}

struct PatternBank {
    patterns: Vec<Pattern>,
}

struct SongBank {
    songs: HashMap<String, Song>,
}

struct Pattern {
    channels: usize,
    steps: usize,
    data: Vec<Vec<Step>>,  // [channel][step]
}

struct Song {
    channels: usize,
    order: Vec<Vec<usize>>,  // [row][channel] → pattern bank index
    loop_point: usize,       // row index to loop back to (0 if no @loop)
}

struct Step {
    cv1: f32,
    cv2: f32,
    trigger: bool,
    gate: bool,
    cv1_end: Option<f32>,  // slide target
    cv2_end: Option<f32>,  // slide target
    repeat: u8,
}
```

The tracker data is wrapped in `Arc<TrackerData>` and attached to the
`ExecutionPlan`:

```rust
struct ExecutionPlan {
    // ... existing fields
    tracker_data: Option<Arc<TrackerData>>,
}
```

Distribution follows the `ReceivesMidi` precedent — a new opt-in trait:

```rust
trait ReceivesTrackerData {
    fn receive_tracker_data(&mut self, data: Arc<TrackerData>);
}
```

Both `MasterSequencer` (which reads song data) and `PatternPlayer` (which
reads pattern data) implement this trait and receive the same
`Arc<TrackerData>`.

**Pattern bank index assignment.** Patterns are assigned bank indices by
alphabetical sort on their names. This ensures indices are stable across
hot-reloads as long as the set of pattern names doesn't change —
reordering `pattern` blocks in the file or editing step data within a
pattern does not affect the index mapping.

On plan activation (audio thread), the plan iterates modules and calls
`receive_tracker_data` on each implementor, passing clones of the `Arc`.
This is a ref-count bump per module — no allocation, no blocking.

On hot-reload, a new plan carries a new `Arc<TrackerData>`. The old plan
(and its modules' `Arc` clones) is sent to the cleanup thread for
deallocation. The audio thread never allocates or frees the data.

**Read-path safety:** `Arc` read access is a plain pointer dereference.
The atomic ref count is only touched on clone (plan activation, once) and
drop (cleanup thread). The audio thread's hot path is array indexing
through the `Arc` — no atomics, no contention.

**Future optimisation: structural sharing.** Currently the entire
`TrackerData` is rebuilt on any change. For typical tracker data
(kilobytes) this is negligible. If live-coding with frequent single-note
edits makes rebuild cost noticeable, the internal structure can move to
persistent data structures — e.g. `Arc<[Arc<Pattern>]>` so that a single
pattern change reuses `Arc` clones for all unchanged patterns. The
top-level `Arc<TrackerData>` is still swapped atomically, but most data
behind it is shared with the previous version. This optimisation is
invisible to modules — they index into `TrackerData` through an opaque
`Arc`, so the internal representation can evolve without changing the
trait or module interface.

### Interpreter validation

The interpreter performs the following checks at build time:

- Every song name referenced by a MasterSequencer's `song` parameter
  exists as a `song` definition.
- Every pattern name referenced in a `song` block exists as a `pattern`
  definition.
- All patterns referenced within a song have the same step count. (This
  determines the rows-per-pattern for the song; no explicit declaration
  needed.)
- Every pattern in a given song column has the same channel count (it will
  be played by the same PatternPlayer, which has a fixed channel count).
- The song's column headers match the declared channels of the
  MasterSequencer that references it.
- The MasterSequencer's song order indices are within the pattern bank's
  bounds.
- Step notation is valid (note literals, floats, slides, repeats parse
  correctly).

**Runtime channel count mismatch.** Because every PatternPlayer receives
the full pattern bank, a player may encounter a pattern with a different
channel count than its own. This is handled gracefully at runtime without
error:

- If the pattern has more channels than the player, the excess channels
  are ignored.
- If the pattern has fewer channels than the player, the player's
  surplus channels are silent (no trigger, gate off).

The interpreter validates channel count consistency for patterns within a
given song column, so mismatches should not occur in normal use. The
runtime behaviour is a safety net, not a designed feature.

### Wiring example

```patches
pattern verse_drums {
    kick:  x . . . x . . . x . . . x . . .
    snare: . . . . x . . . . . . . x . x .
}

pattern bass_a {
    note: C2 . . ~ E2 . G2 . C2 . . ~ A1 . . .
    vel:  1.0 . . ~ 0.8 . 0.6 . 1.0 . . ~ 0.7 . . .
}

song my_song {
    | drums       | bass   |
    | verse_drums | bass_a |
    | verse_drums | bass_a |
}

patch {
    module seq: MasterSequencer(channels: [drums, bass]) {
        song: my_song, bpm: 120, rows_per_beat: 4
    }
    module drums: PatternPlayer(channels: [kick, snare])
    module bass: PatternPlayer(channels: [note, vel])

    seq.clock[drums] -> drums.clock
    seq.clock[bass]  -> bass.clock

    drums.trigger[kick]  -> kick_synth.trigger
    drums.cv2[kick]      -> kick_synth.velocity
    drums.trigger[snare] -> snare_synth.trigger

    bass.cv1[note]    -> bass_osc.voct
    bass.gate[note]   -> bass_env.gate
    bass.trigger[note] -> bass_env.trigger
    bass.cv1[vel]     -> bass_vca.cv
}
```

---

## Alternatives considered

### Channel kinds (note, drum, param)

PatternPlayer outputs would vary per channel based on a declared kind —
note channels produce voct/trigger/gate/velocity, drum channels produce
only trigger, param channels produce only cv.

Rejected: this leaks DSL sugar into the module descriptor layer. The
existing alias mechanism (`channels: [kick, snare]` desugars to a count
with locally-bound indices) has no concept of per-channel types, and
adding one would be a new dimension of complexity in the descriptor,
interpreter, and planner. Uniform outputs with connected-port
optimisation achieve the same efficiency without the abstraction cost.

### Per-player pattern lists

Each PatternPlayer declares which patterns it can play:
`patterns: [verse_drums, fill_drums]`. The interpreter builds a per-player
bank.

Rejected: requires either explicit binding in the DSL (extra
configuration surface) or wiring analysis (the interpreter traces clock
connections to infer which player receives which patterns). Both are
fragile. Broadcasting the full tracker data to all players is simpler —
each player just indexes by the bank index on its clock bus. The data is
`Arc`-shared, so memory cost is a single allocation regardless of player
count.

### Inline song data on MasterSequencer

The song order is a parameter on the MasterSequencer module rather than a
separate `song` block.

Rejected: the song order is a 2D table of pattern references that maps
poorly to existing parameter types. A dedicated `song` block makes the
arrangement visually scannable and keeps the patch block focused on
signal-flow wiring.

### Separate clock and pattern-select cables

Instead of a poly clock bus, use separate mono outputs for tick trigger,
tick duration, and pattern select.

Rejected: the poly bus is a single cable carrying all timing and selection
data. This keeps wiring minimal (one connection per PatternPlayer) and
is extensible — additional control signals (e.g. song position, loop
flag) can be added to unused voices without changing the wiring topology.

---

## Consequences

- **Two new top-level DSL constructs** (`pattern`, `song`) must be added
  to the grammar, parser, AST, and interpreter. These are substantial
  grammar extensions but are syntactically isolated from existing
  constructs.
- **Two new modules** (`MasterSequencer`, `PatternPlayer`) are added to
  `patches-modules`. `PatternPlayer` is generic and reusable;
  `MasterSequencer` is coupled to the `song` block semantics.
- **New module trait** (`ReceivesTrackerData`) and a new field on
  `ExecutionPlan`. The planner must broadcast tracker data during plan
  activation, following the `ReceivesMidi` precedent.
- **`Arc<TrackerData>` on the plan.** Plan size increases by the size of
  pattern and song data. For typical tracker data (16–64 steps, 2–8
  channels, tens of patterns) this is negligible — kilobytes at most.
- **Hot-reload works naturally.** Editing a pattern or the song order
  produces a new plan with a new `Arc<TrackerData>`. The old data is freed
  on the cleanup thread. Modules in the new plan receive the updated
  tracker data at activation.
- **No impact on existing modules or wiring.** The pattern/song system is
  fully additive. Patches without `pattern` or `song` blocks are
  unaffected.
