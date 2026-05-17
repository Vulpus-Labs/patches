# Anatomy of a self-contained patch

This chapter walks through `examples/tracker_three_voices.patches`,
a piece for three voices — bass, two leads — driven entirely by the
tracker sequencer. No external MIDI; the patch *is* the composition.

If [Anatomy of a synth voice](authoring-synth-voice.md) shows how a
voice produces sound, this chapter shows how the sequencer drives it.

The patch is over 100 lines and full of detail; we'll read it in
chunks. The full file is at `examples/tracker_three_voices.patches`.

## Three things at once

A self-contained piece typically has three layers:

1. **The composition.** `song` and `pattern` blocks declaring what
   notes play when. Top of the file, before the patch block.
2. **The voice machinery.** `template` blocks that wrap synth-voice
   wiring so the patch can stamp out one per voice. Also top-of-file.
3. **The patch.** A `MasterSequencer`, one `PatternPlayer` per
   voice, instances of the voice templates, mixer, effects, output.

Reading order matters: skim top to bottom for the lay of the land
(songs → templates → patch), then re-read each layer in detail.

## The song

```patches
song demo(bass, lead1, lead2) {
    pattern bass_verse {
        note: C2  .  .  ~  Eb2 .  G1  .  C2  .  .  ~  Ab1 .  .  .
        vel:  1.0 .  .  ~  0.8 .  0.9 .  1.0 .  .  ~  0.7 .  .  .
    }

    pattern bass_chorus { ... }
    pattern lead1_verse { ... }
    pattern lead1_chorus { ... }
    pattern lead2_verse { ... }
    pattern lead2_chorus { ... }
    pattern lead2_fill  { ... }

    play {
        (bass_verse,  lead1_verse,  lead2_verse
        bass_verse,  lead1_verse,  lead2_verse) * 2
        (bass_chorus, lead1_chorus, lead2_chorus
        bass_chorus, lead2_chorus,   lead2_fill) * 2
    }
}
```

A `song` declares its voices (`demo(bass, lead1, lead2)`). The
patterns inside reference those voice names. The `play { ... }`
block lists which patterns are active simultaneously for each row
of the song, with `(...) * N` for repetition.

### Pattern syntax in one breath

- `note:` and `vel:` are *channel rows*. The names are conventions;
  any identifier works. Each step on a row is one slot.
- `C2`, `Eb2`, `G1`, `Ab1` — note names with octave.
- `.` — rest. Silence on this step; the previous gate ends.
- `~` — tie. Hold the previous note's gate across this step
  without re-triggering.
- `1.0`, `0.8` — bare floats. Used on `vel:` for velocity.

Step counts must match across rows in a pattern. Here every row has
16 steps; the sequencer treats them as 16 sixteenth-notes (with
`rows_per_beat: 4`, four steps per beat = sixteenth notes).

Slides (`C4>Eb4`), continues (`x`), and repeats (`*N`) extend the
basic syntax — see the [DSL reference](dsl-reference.md) under
*Patterns* and *Songs* for the full set.

### The play block

```patches
play {
    (bass_verse,  lead1_verse,  lead2_verse
    bass_verse,  lead1_verse,  lead2_verse) * 2
    (bass_chorus, ...) * 2
}
```

A *row* names which pattern plays on each voice for the next chunk
of song time. Commas separate columns within a row; newlines
separate rows. Parenthesised groups can be repeated with `* N`.

Reading the first group: row 1 plays `bass_verse` on bass,
`lead1_verse` on lead1, `lead2_verse` on lead2; row 2 plays the
same three again; the group repeats twice — so four iterations of
"verse" before the chorus block starts.

## The voice template

```patches
template voice(attack: float = 0.01, decay: float = 0.1,
               sustain: float = 0.7, release: float = 0.001,
               cutoff: float = 6.0, q: float = 0.0,
               glide_ms: float = 0.0) {
    in:  voct, gate, trigger, velocity
    out: audio

    module osc    : Osc
    module env    : Adsr { attack: <attack>, decay: <decay>,
                           sustain: <sustain>, release: <release> }
    module vca    : Vca
    module filt   : Svf { cutoff: <cutoff>, q: <q> }
    module vel_vca: Vca
    module glide  : Glide { glide_ms: <glide_ms> }

    $.voct      -> glide.in
    glide.out   -> osc.voct
    $.gate      -> env.gate
    $.trigger   -> env.trigger

    osc.sawtooth -> filt.in
    filt.lowpass -> vca.in
    env.out      -> vca.cv
    vca.out      -> vel_vca.in
    $.velocity   -> vel_vca.cv
    vel_vca.out  -> $.audio
}
```

A monophonic voice (not poly — see below) with the standard
oscillator → filter → VCA → velocity-VCA chain. Differences from
the previous chapter's lead voice:

- **Monophonic.** No `Poly`* prefix anywhere. The tracker plays one
  note per voice per step; there is no polyphony required.
- **Glide.** `Glide` smooths the pitch CV — when a step plays a
  different note from the previous step, the pitch slews to it
  instead of jumping. `glide_ms: 0.0` disables it.
- **Velocity VCA.** The pattern's `vel:` row drives a second VCA
  *after* the amp envelope, so velocity scales the dynamic shape.
- **`Svf` filter.** State-variable filter with `cutoff` in V/oct
  units. Cheaper than the lead voice's `PolyLowpass`.

The template's interface is `voct, gate, trigger, velocity` in and
`audio` out — the same shape `PolyMidiToCv` would feed, but mono.

A second template (`sq_voice`) is a square-wave variant for `lead2`
— same skeleton, swapped oscillator output and different defaults.

## The patch

The actual graph. Read it top-down:

### Sequencer + pattern players

```patches
module seq: MasterSequencer([bass, lead1, lead2]) {
    song: demo,
    bpm: 120,
    rows_per_beat: 4,
    loop: true,
    autostart: true,
    swing: 0.75
}

module bass_pp:  PatternPlayer([note, vel])
module lead1_pp: PatternPlayer([note, vel])
module lead2_pp: PatternPlayer([note, vel])

seq.clock[bass]  -> bass_pp.clock
seq.clock[lead1] -> lead1_pp.clock
seq.clock[lead2] -> lead2_pp.clock
```

`MasterSequencer` consumes the song (`song: demo`), runs at
`bpm: 120` with 4 rows per beat (sixteenth notes), loops the play
block, autostarts on load, and applies 0.75 swing.

For each voice it emits a `clock[<voice>]` output: a stream of
"row events" that name which pattern is active, which step is
current, and the step's data.

`PatternPlayer([note, vel])` listens to a clock and unpacks the two
named channels — `note` and `vel`. It exposes per-channel outputs:

- `pp.cv1[note]` — the pitch CV from the `note:` row.
- `pp.gate[note]` — gate held high during notes (low during `.`).
- `pp.trigger[note]` — single-sample pulse at step start.
- `pp.cv1[vel]` — the velocity CV from the `vel:` row.

The names `cv1`, `gate`, `trigger` are the `PatternPlayer`'s
fixed port set; the `[name]` index is the channel from the
pattern.

### Voices

```patches
module bass_v: voice(
    attack: 0.005, decay: 0.15, sustain: 0.6, release: 0.1,
    cutoff: 3.5, q: 0.3, glide_ms: 40.0
)

bass_pp.cv1[note]     -> bass_v.voct
bass_pp.gate[note]    -> bass_v.gate
bass_pp.trigger[note] -> bass_v.trigger
bass_pp.cv1[vel]      -> bass_v.velocity
```

Bass uses the `voice` template with 40ms glide for portamento
between notes, a tight envelope, and a moderately low filter
cutoff. `lead1_v` and `lead2_v` instantiate the same shape with
different parameters; `lead2_v` uses `sq_voice` for a different
oscillator waveform.

### Mixer with sends

```patches
module mix: StereoMixer([bass, lead1, lead2]) {
    @bass:  { level: 0.7, pan:  0.0 },
    @lead1: { level: 0.5, pan: -0.4, send_a: 0.3 },
    @lead2: { level: 0.5, pan:  0.4, send_a: 0.3 }
}

bass_v.audio  -> mix.in[bass], ~meter+osc+spectrum(bass)
lead1_v.audio -> mix.in[lead1]
lead2_v.audio -> mix.in[lead2]
```

Three-channel stereo mixer. Bass is centred and loudest; the two
leads are softer, panned left and right, with a 30% send to bus A.

The `~meter+osc+spectrum(bass)` at the end of `bass_v.audio ->`
attaches a *tap* to the cable, exposing live meter / scope /
spectrum data to the GUI. See [Visualising patches](authoring-visualising.md)
for taps.

### Effects bus

```patches
module delay: StereoDelay(2) {
    delay_ms: 750, feedback: 0.4, dry_wet: 1.0
}
mix.send_a  -> delay.in
delay.out   -> mix.return_a

module reverb: FdnReverb {
    size: 0.45, brightness: 0.6, mix: 0.2, character: plate
}
module lim: StereoLimiter { threshold: 0.85 }

mix.out    -> reverb.in
reverb.out -> lim.in

module out: AudioOut
lim.out -> out.in
```

Two effects layers:

1. **Send-return.** `mix.send_a` carries the delay-bound signal
   from the leads (no bass). The delay's wet-only output
   (`dry_wet: 1.0`) returns to `mix.return_a`, which the mixer
   sums into its main output. Standard send-return topology.
2. **Master bus.** `mix.out` (everything: voices + return) → reverb
   → limiter → `AudioOut`. The reverb is plate-flavoured with
   moderate mix.

## Design notes

A few decisions worth explaining:

**Monophonic voices over poly.** The sequencer plays one note per
voice per step. There's no polyphony to allocate, so each voice is
a mono chain — cheaper, simpler, and the template's parameter list
controls per-voice character without any shared poly cost.

**`Svf` over `Lowpass`.** State-variable filter is cheaper than a
biquad cascade and the leads don't need the steeper rolloff. Bass
might benefit from a steeper filter; an exercise for the reader.

**Pattern length 16.** With `rows_per_beat: 4` that's exactly four
beats per pattern. Different pattern lengths within a song are
fine (some voices can sustain across multiple steps via `~`), but
keeping them aligned to common multiples simplifies the timing.

**Swing on the sequencer, not per-voice.** `swing: 0.75` shifts
off-beats slightly later for groove. Applying it at the sequencer
means every voice swings in sync.

**Autostart in the player; transport-driven in a DAW.** With
`autostart: true`, the piece plays as soon as the patch loads in
`patch_player`. If you load it as a CLAP plugin in a DAW, you may
prefer `autostart: false` and wire `seq.start` to the host's
transport — but that's a host-integration decision, not part of the
patch itself.

## Mixing the modes

There's no rule preventing a sequenced backing layer from coexisting
with an externally-driven lead. A `MasterSequencer` driving a few
voices and a `PolyMidiToCv` driving a lead voice can live in the
same patch — the engine doesn't care where the gate edges come from.

Self-contained vs. externally-driven is a property of the wiring,
not a setting on the player.
