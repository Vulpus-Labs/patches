# Self-contained patches

A *self-contained patch* generates its own notes. It uses the
tracker, step-sequencer, or clock modules as the note source rather
than reading MIDI from an external controller or a DAW. Load it in
the player, the piece starts, and the patch is the composition.

This is the third of the three peer workflows. The DSL, modules, and
engine are identical to the other two — the only difference is what
drives the gate, trigger, and pitch ports of the voice modules.

## How it works

External-MIDI patches typically begin with something like:

```patches
module kbd : PolyMidiToCv
kbd.voct    -> osc.voct
kbd.gate    -> env.gate
kbd.trigger -> env.trigger
```

A self-contained patch replaces that source with a sequencer, and
wires the sequencer's clocks and CV outputs into one or more voices:

```patches
module seq: MasterSequencer([voice_a]) {
    song: my_song,
    bpm: 120,
    rows_per_beat: 4,
    autostart: true
}
module pp: PatternPlayer([note, vel])

seq.clock[voice_a] -> pp.clock
pp.cv1[note]       -> voice.voct
pp.gate[note]      -> voice.gate
pp.trigger[note]   -> voice.trigger
pp.cv1[vel]        -> voice.velocity
```

The notes themselves live in `song` and `pattern` blocks at the top
of the file. Their full syntax — pitch names, ties (`~`), continues
(`.`), velocity rows, transitions (`>`), repeats (`*N`),
`play { ... }` structure — is covered in the
[DSL reference](dsl-reference.md) under the tracker section.

## Walkthrough — `tracker_three_voices.patches`

`examples/tracker_three_voices.patches` is a three-voice piece: bass,
two leads, stereo delay and reverb. Run it:

```bash
patch_player examples/tracker_three_voices.patches
```

It plays itself. No MIDI connection needed. Ctrl-C to stop.

The structure, top to bottom:

1. **`song demo(bass, lead1, lead2)`** — declares three named
   voices and the patterns they play. Each `pattern` block defines a
   row of pitches and a row of velocities; `~` ties into the previous
   note, `.` continues the gate, `*N` repeats. `play { ... }` lists
   the patterns to chain together, with `* N` for repetition.
2. **`template voice(...)`** — a parameterised synth-voice template
   exposing `voct`, `gate`, `trigger`, `velocity` inputs and one
   audio output. Internally: oscillator, ADSR, VCA, filter, glide.
3. **`template sq_voice(...)`** — a square-wave variant for lead 2.
4. **`patch { ... }`** — the actual graph. Inside:
   - `MasterSequencer` consumes the song and produces a per-voice
     clock plus pattern data.
   - One `PatternPlayer` per voice unpacks the active pattern's
     rows into CV / gate / trigger streams.
   - Each voice template instantiates with per-voice parameters
     (attack, decay, filter cutoff, glide).
   - `StereoMixer(3)` sums voices, panning leads left and right.
   - A delay send/return loop and a master reverb + limiter sit on
     the bus.

The bottom of the file is the same `AudioOut` the player chapter
showed — the only difference is that audio reaching it was generated
internally rather than triggered from outside.

## Autostart vs. external clock

`MasterSequencer` accepts `autostart: true` (start playing when the
patch loads) or `false` (wait for an external trigger). In the
player, with `autostart: true`, the patch plays as soon as the file
is loaded. In a DAW, you can wire the host's transport into the
sequencer's start input for project-synchronised playback — see the
sequencer entry in [Sequencers & clocks](modules/sequencers.md).

## Mixing the modes

Nothing prevents a single patch from doing both. A `MasterSequencer`
can drive a backing voice while a `PolyMidiToCv` reads incoming MIDI
into a lead voice — the engine doesn't care where the gate edges
come from. Self-contained vs. externally-driven is a property of how
the patch is wired, not a mode the player has to be told about.
