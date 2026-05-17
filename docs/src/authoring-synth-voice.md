# Anatomy of a synth voice

This chapter walks through `examples/synths/lead.patches`, a
sixteen-voice polyphonic lead synth driven by MIDI. It's the canonical
"build a synth voice from oscillator outward" patch. The architecture
generalises: pad, bass, pluck, brass — every voice in `examples/synths/`
is a variation on the same skeleton with different envelope and filter
choices.

This is patch authoring, not DSL syntax. If you want a refresher on
how `module` declarations and connections work, read [DSL
basics](dsl-basics.md) first.

## The full patch

```patches
patch {
    module kbd      : PolyMidiToCv
    module osc      : PolyOsc { drift: 0.04 }
    module mix      : PolySum([saw, sq])
    module amp_env  : PolyAdsr {
        attack: 0.003, decay: 0.18, sustain: 0.7, release: 0.18
    }
    module vca      : PolyVca
    module filt_env : PolyAdsr {
        attack: 0.005, decay: 0.22, sustain: 0.4, release: 0.2
    }
    module filt     : PolyLowpass {
        cutoff: 1.2kHz, resonance: 0.55, saturate: true
    }
    module delay    : StereoDelay {
        delay_ms: 280, feedback: 0.35, tone: 0.7, dry_wet: 0.25,
        pingpong: true
    }
    module lim      : StereoLimiter { threshold: -3db }
    module out      : AudioOut

    kbd.voct        -> osc.voct
    kbd.trigger     -> amp_env.trigger
    kbd.gate        -> amp_env.gate
    kbd.trigger     -> filt_env.trigger
    kbd.gate        -> filt_env.gate

    osc.sawtooth -[0.6]-> mix.in[saw]
    osc.square   -[0.5]-> mix.in[sq]
    mix.out         -> vca.in
    amp_env.out     -> vca.cv

    vca.out         -> filt.in
    filt_env.out -[1.0]-> filt.voct

    filt.out     -[0.1]-> delay.in
    delay.out       -> lim.in
    lim.out         -> out.in
}
```

Load it as a CLAP plugin on a MIDI track (or via `patch_player` with
a connected MIDI controller). Play. The rest of this chapter is the
explanation.

## Signal flow

A classic subtractive architecture: oscillator → mixer → amplifier →
filter → effects → output. Every stage from `osc` through `filt` is
polyphonic, so each of the 16 voices has its own independent signal
path. After the filter, the `StereoDelay` and `StereoLimiter`
operate on a stereo bus — they don't need to be per-voice.

```text
kbd ──voct────→ osc ──saw────→ mix ──→ vca ──→ filt ──→ delay ──→ lim ──→ out
                      ──sq────↗        ↑       ↑
                                   amp_env  filt_env
```

The mono-to-stereo step happens *implicitly* at `filt.out -> delay.in`:
`delay.in` is a stereo input, and a mono audio source into a stereo
input silently broadcasts to both channels. No explicit `MonoToStereo`
needed.

## MIDI input

`PolyMidiToCv` is the source of poly traffic for the whole patch. It
receives MIDI events from the host (DAW or `patch_player`), allocates
each note-on to a voice slot (LIFO stealing if all 16 are busy), and
produces three poly outputs:

- **voct** — pitch as V/oct (1 unit per octave). Drives `osc.voct`.
- **gate** — `1.0` while a key is held, `0.0` when released. Tells
  the envelopes when to enter their release phase.
- **trigger** — single-sample pulse on note-on. Tells the envelopes
  when to restart from the attack phase.

The gate / trigger distinction matters. The gate tells the envelope
*how long* the note is held; the trigger tells it *when* to restart.
With only a gate, restriking the same note while the previous
release is still falling wouldn't retrigger — the gate would stay
high through both presses.

For DAW use, the host writes its track's MIDI directly to the
plugin; you don't think about device selection.

## Oscillator and mixing

`PolyOsc` produces multiple waveforms in parallel — sine, sawtooth,
square, triangle — at the pitch set by `voct`. This patch blends two:

```patches
osc.sawtooth -[0.6]-> mix.in[saw]
osc.square   -[0.5]-> mix.in[sq]
```

The scale factors are the blend ratio: 60% sawtooth, 50% square. The
total (1.1) is a little hot deliberately — the limiter at the end of
the chain catches the peaks while keeping the bright bite of both
shapes.

`PolySum([saw, sq])` is a two-channel summing mixer with *named*
inputs. The names `saw` and `sq` are arbitrary — readable labels for
the connections. Numeric indices (`PolySum(2)`, `mix.in[0]`) work
just as well.

`drift: 0.04` on `PolyOsc` adds a small random per-voice detune that
makes massed voices sound fuller. Set it to `0.0` for a perfectly
in-tune (sterile-sounding) stack; raise it for thicker detuning.

## Amplitude shaping

Without a VCA, every voice would sound continuously. The amplitude
envelope shapes the dynamics of each note:

```patches
kbd.trigger -> amp_env.trigger
kbd.gate    -> amp_env.gate
mix.out     -> vca.in
amp_env.out -> vca.cv
```

`PolyAdsr` produces a per-voice control signal that:

1. Rises to `1.0` over `attack` seconds when triggered.
2. Falls to `sustain` (`0.7`) over `decay` seconds.
3. Holds at `sustain` while the gate is high.
4. Falls to `0.0` over `release` seconds on gate-off.

`PolyVca` multiplies the audio by the envelope. Output is `0` when
the envelope is `0` (silent voice), `audio` when the envelope is `1`
(full-amplitude voice).

The values here — fast `0.003` (3 ms) attack, moderate `0.18` decay,
high `0.7` sustain, short `0.18` release — give a punchy, plucked feel
that still holds notes for sustained chords. Automate the host
controls on these parameters from a DAW track to morph the envelope
shape over a phrase.

## Filter and filter envelope

The filter sits *after* the VCA, which is the unusual choice here —
classic synth topologies often put it before. Putting it after means
the filter sees the amplitude-shaped voice, so filter envelope
modulation interacts with the amp envelope rather than being
independent.

```patches
vca.out         -> filt.in
filt_env.out -[1.0]-> filt.voct
```

`filt.voct` is a V/oct *cutoff* input — the filter cutoff is set in
V/oct just like an oscillator's pitch. The filter envelope adds to
the base cutoff (`1.2kHz`) in V/oct units, opening the filter on note
attack and closing it during release.

`-[1.0]-> filt.voct` is full-depth modulation. The filter envelope's
own shape (slow attack, moderate decay, low sustain) gives the
characteristic "pluck" — bright on the onset, then darker. Lower the
scale (`-[0.5]->`) for a subtler sweep; raise it (`-[2.0]->`) for an
aggressive, almost over-the-top opening.

`resonance: 0.55` adds emphasis at the cutoff frequency — nasal and
slightly vocal. `saturate: true` enables soft-clipping inside the
filter, adding warmth when driven hard. Both are good targets for
host automation.

## Effects bus

The filter feeds a stereo delay and a stereo limiter:

```patches
filt.out -[0.1]-> delay.in
delay.out       -> lim.in
lim.out         -> out.in
```

The `-[0.1]->` scaling into the delay drops the level by ~20 dB
because the limiter at the end of the chain is set to `-3dB`
threshold — too hot a signal hits the limiter constantly and loses
its dynamics. With this scaling the delay sits at a comfortable level
under the dry voice.

`StereoDelay` operates on a stereo bus: its `in` and `out` are stereo
ports. The mono → stereo broadcast at `filt.out -> delay.in` is
implicit; the delay itself can produce stereo movement via
`pingpong: true`, which alternates taps between left and right.

The `StereoLimiter` catches the cumulative peaks of 16 voices and
prevents clipping at the AudioOut.

## What to automate from a DAW

The natural automation targets for this voice, in the order you'd
typically reach for them:

| Parameter            | Expression                          |
| -------------------- | ----------------------------------- |
| `filt.cutoff`        | Brightness sweeps.                  |
| `filt.resonance`     | Squelch / vocal emphasis.           |
| `delay.dry_wet`      | Effect amount.                      |
| `amp_env.attack`     | Soft / hard onsets.                 |
| `amp_env.release`    | Short / sustained tails.            |
| `mix.in[saw]` scale  | Harmonic balance — saw vs square.   |

To make these available as host parameters, declare them as host
controls in the patch (see [DSL reference](dsl-reference.md), *Host
controls*). They then show up in the DAW's parameter list for
automation, modulation, and per-track recall.

## Variations

The other patches under `examples/synths/`:

- **`pad.patches`** — slow attack, two detuned oscillator layers,
  long hall reverb. Same skeleton; pad-shaped envelopes.
- **`bass.patches`** — monophonic-feeling poly with a low-pass set
  for sub frequencies, tight envelopes.
- **`pluck.patches`** — almost no sustain, fast filter envelope,
  short delay.
- **`bell.patches`**, **`fm_pad.patches`** — FM-style spectra,
  longer envelopes.

Reading two or three of these side-by-side is the fastest way to see
which parameters carry the character of a sound and which are
incidental.

## Hot-reload while authoring

When you save the file, the engine swaps in the new graph without
stopping audio. Modules whose name and type are unchanged carry
their state — `osc`'s phase, `delay`'s buffer, `amp_env`'s position
all survive. This is the [edit-reload cycle](authoring-edit-reload.md):
treat it as a REPL for iteratively shaping a sound rather than as a
performance feature.
