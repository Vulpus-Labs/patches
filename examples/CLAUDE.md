# Writing `.patches` files — Claude context

This directory holds example patches for the Patches DSL. Read this before
writing new patches for a user. The other `.patches` files here are the
authoritative reference for idiom — when in doubt, grep them.

## What a .patches file is

A patch is a signal graph of audio modules wired with cables. The file is
parsed by `patches-dsl`, expanded (templates → concrete modules), then
executed by the engine. Core execution rule: **every cable delays by one
sample**, so feedback loops and module order are always safe.

Two key design affordances the DSL offers — **use them**:

1. **Templates** — parameterised sub-patches. Use these to isolate voice
   implementation from instrument-level orchestration. The top-level
   `patch { ... }` block should read as "how the voices compose", not
   "which oscillators and filters exist".
2. **Channel aliases** — shape args like `channels: [kick, snare, hat]`
   give you named indices instead of `in[0]`, `in[1]`, `in[2]`. Always
   prefer aliases once you have more than two channels.

## File structure

```
# optional — pull in other files (resolved relative to current file)
include "instruments.patches"

# zero or more
template name(params) { in: ...; out: ...; <body> }
pattern name { track: x . x . ... }
song name(tracks) { play { ... } }

# exactly one, at the end
patch {
    module <name> : <Type>[(shape)] [{ params }]
    <source>.port -[scale]-> <dest>.port
}
```

Comments are lines starting with `#`. Inline comments after tokens are not
supported — keep them on their own line.

## Wiring syntax

```
src.port -> dst.port                 # plain cable
src.port -[0.5]-> dst.port           # scaled cable
src.port -[-6db]-> dst.port          # unit-suffixed scale
dst.port <- src.port                 # reverse arrow (equivalent)
src.port -> a.in, b.in               # fan-out is implicit: just wire twice
```

Indexed ports: `mix.in[0]` or `mix.in[kick]` (alias). `$.foo` refers to the
enclosing template's external port (input or output).

## Value literals

| Form | Example | Meaning |
|------|---------|---------|
| float | `0.5`, `-1.2` | plain number |
| int | `4` | integer |
| bool | `true`, `false` | boolean |
| note | `C4`, `F#3`, `Bb0` | MIDI pitch as V/oct |
| freq | `440Hz`, `1.2kHz` | frequency |
| dB | `-6dB`, `0.5dB` | decibels |
| semi | `3s`, `-1s` | semitones |
| cents | `50c`, `-25c` | cents |
| enum | `linear`, `sawtooth` | bare identifier |
| string | `"path/to/ir.wav"` | quoted |
| param ref | `<attack>` | inside templates, substitutes a template param |

V/oct convention: `0.0` ≈ 16.35 Hz (C0); `+1.0` = one octave up; one
semitone = `1/12`.

## Poly vs mono cables

- Mono modules: one channel. Names like `Osc`, `Lfo`, `Vca`, `Adsr`.
- Poly modules: N independent voices. Names prefixed `Poly*`
  (`PolyOsc`, `PolyVca`, `PolyAdsr`, `PolyLowpass`, `PolySvf`, …).
- `PolyMidiIn` produces poly `voct`, `gate`, `trigger`, `velocity`.
- `MonoToPoly` broadcasts a mono signal to every voice. Use this for LFOs
  modulating a poly oscillator: `lfo.sine -> m2p.in; m2p.out -> osc.fm`.
- `PolyToMono` sums all voices to mono. Always follow with attenuation
  (`-[0.1]->` or similar) before the output — N voices can easily clip.

## Templates — the core abstraction

```
template voice(
    attack: float = 0.01, decay: float = 0.1,
    sustain: float = 0.7, release: float = 0.3,
    cutoff: float = 6.0, q: float = 0.0,
    glide_ms: float = 0.0
) {
    in:  voct, gate, trigger, velocity
    out: audio

    module osc   : Osc
    module env   : Adsr { attack: <attack>, decay: <decay>,
                          sustain: <sustain>, release: <release> }
    module filt  : Svf  { cutoff: <cutoff>, q: <q> }
    module vca   : Vca
    module glide : Glide { glide_ms: <glide_ms> }

    $.voct    -> glide.in
    glide.out -> osc.voct
    $.gate    -> env.gate
    $.trigger -> env.trigger

    osc.sawtooth -> filt.in
    filt.lowpass -> vca.in
    env.out      -> vca.cv
    vca.out      -> $.audio
}
```

Types: `float`, `int`, `bool`, `str`, `pattern`, `song`. Use parameter
references `<name>` inside module parameter blocks to thread them through.
Port groups can take arity: `in: inputs[n_inputs]` with
`n_inputs: int = 3`.

**Rule of thumb: a new user request for a synth means a new template per
distinct voice.** Lead, bass, pad, drum kit — each gets its own template
with sensibly named parameters. The `patch { }` block instantiates them,
wires keyboard/sequencer to them, and mixes their outputs. It should be
readable as a block diagram. See
[poly_synth_layered.patches](poly_synth_layered.patches),
[tracker_three_voices.patches](tracker_three_voices.patches),
[song1/instruments.patches](song1/instruments.patches) for this pattern
applied seriously.

## Channel aliases

For anything with >2 channels, name them:

```
module mix : StereoMixer(channels: [lo, hi, noise]) {
    level[lo]: 0.7, level[hi]: 0.5, level[noise]: 0.3,
    pan[lo]: -0.4, pan[hi]: 0.4,
    send_a[hi]: 0.5
}
module seq : MasterSequencer(channels: [kick, snare, hat, bass, lead])
seq.clock[bass] -> bass_pp.clock
```

Indexed parameter blocks:

```
module del : StereoDelay(channels: [early, late]) {
    @early: { delay_ms: 250, feedback: 0.3 },
    @late:  { delay_ms: 500, feedback: 0.4 }
}
```

## Patterns and songs (sequencer-driven patches)

```
pattern kick_basic {
    hit: x:1.0 .     .     .     x:1.0 .     .     .
}
pattern bassline {
    note: C2 . Eb2 . G2 . Bb2 .
    vel:  0.9 . 0.7 . 0.9 . 0.7 .
}

song tune(kick, bass) {
    play {
        kick_basic, bassline
        kick_basic, bassline
    }
}
```

`x:0.8` is a hit with velocity 0.8. `.` is silence. Groups can repeat
with `(...) * 2`. Notes use MIDI note-name literals. The tracks declared
in the song header (`kick, bass`) must match `MasterSequencer`'s channel
names.

Wiring a sequenced voice:

```
module seq : MasterSequencer(channels: [bass]) { song: tune, bpm: 120, rows_per_beat: 4 }
module bass_pp : PatternPlayer(channels: [note, vel])
module bass_v  : voice(glide_ms: 30.0, cutoff: 4.0)

seq.clock[bass]      -> bass_pp.clock
bass_pp.cv1[note]    -> bass_v.voct
bass_pp.trigger[note]-> bass_v.trigger
bass_pp.gate[note]   -> bass_v.gate
bass_pp.cv2[note]    -> bass_v.velocity   # velocity ends up on cv2 by convention
bass_v.audio         -> mix.in[bass]
```

Exact `PatternPlayer` output port names vary by how channels are declared
— check a working example (`tracker_three_voices.patches`,
`song1/song.patches`) before guessing.

## Available modules

Listed by source file under `patches-modules/src/`. Each module's doc
comment is the source of truth for ports/parameters — read it if you need
precise types. Names here are the type names used in the DSL.

**Oscillators / sources:** `Osc`, `PolyOsc`, `Noise`, `PolyNoise`, `Lfo`,
`PolyLfo`.

**Filters:** `Lowpass`, `Highpass`, `Bandpass`, `Svf`, and their `Poly*`
versions. `Svf` gives simultaneous `lowpass`/`highpass`/`bandpass`
outputs and is the default choice when you want multimode.

**Envelopes / shaping:** `Adsr`, `PolyAdsr`, `Vca`, `PolyVca`, `Glide`.

**Mixing / routing:** `Sum`, `PolySum`, `Mixer`, `StereoMixer`,
`MonoToPoly`, `PolyToMono`.

**Pitch:** `Tuner`, `PolyTuner` (octave/semi/cent offset on V/oct),
`Quant`, `PolyQuant` (scale quantisers), `PolySah` / `Sah` (sample &
hold).

**Effects:** `Delay`, `StereoDelay`, `FdnReverb`, `ConvolutionReverb` (if
present), `RingMod`, `Drive`, `Bitcrusher`, `TransientShaper`,
`PitchShift`, `Limiter`, `StereoLimiter`.

**Drum voices** (each takes `trigger`, `velocity`): `Kick`, `Snare`,
`ClosedHiHat`, `OpenHiHat`, `Tom`, `Clap`, `Claves`, `Cymbal`.

**I/O:** `AudioIn`, `AudioOut`.

**MIDI / sequencing:** `PolyMidiIn`, `MidiCc`, `MasterSequencer`,
`PatternPlayer`, `HostTransport`, `MsTicker`, `TempoSync`.

Before using a module with uncommon parameters, grep
`patches-modules/src/<module>.rs` for its descriptor / doc comment to
confirm port and parameter names. Don't guess.

## Conventions (match these)

- **Stereo ports use `_left` / `_right` suffixes**: `in_left`, `out_right`,
  `send_a_left`, `return_b_right`. Never `in_l` or `inL`.
- **Trigger vs gate**: `trigger` is a one-sample pulse on note-on;
  `gate` is held high for the note duration. ADSR needs both.
- **Attenuate poly sums**: after `PolyToMono`, scale by ~`0.1` for 16
  voices, more for fewer. Clipping is usually a missing attenuator.
- **Resonance on lowpasses**: parameter name is often `resonance` (biquad
  lowpasses) or `q` (SVF). Not interchangeable.
- **One template per conceptual instrument**, with parameters for the
  things a user would want to tweak (attack, cutoff, glide, etc.). Don't
  hard-code numbers that differ between instances.

## Minimal skeleton

```
patch {
    module kbd  : PolyMidiIn
    module osc  : PolyOsc
    module env  : PolyAdsr { attack: 0.01, release: 0.3 }
    module vca  : PolyVca
    module coll : PolyToMono
    module out  : AudioOut

    kbd.voct    -> osc.voct
    kbd.trigger -> env.trigger
    kbd.gate    -> env.gate
    osc.sawtooth -> vca.in
    env.out      -> vca.cv
    vca.out      -> coll.in
    coll.out -[0.1]-> out.in_left
    coll.out -[0.1]-> out.in_right
}
```

## Tooling — use these instead of guessing

Two CLI tools live in the `patches-tools` crate. Run them from the repo
root. Prefer them over trusting this document's module list, which can go
stale.

### `patches-check` — validate a patch file

```sh
cargo run -q -p patches-tools --bin patches-check -- <file.patches>
```

Runs the full parse → expand → bind → interpret pipeline and prints one
diagnostic per line:

```
path:line:col: error: [BN0001] unknown module 'VChorus'
  ^ unknown module type
```

Exit code is 1 if any error diagnostic was produced, 0 otherwise
(warnings alone do not fail). Run it after every non-trivial edit — it is
the ground truth for module names, port names, parameter types, and
connectivity rules.

Pass `--module-path <dir-or-file>` (repeatable) to include FFI plugin
bundles during validation.

### `patches-manifest` — list registered modules

```sh
cargo run -q -p patches-tools --bin patches-manifest
```

Dumps every module in the registry with its `ModuleDescriptor`: shape,
inputs (with cable kind: `mono` / `poly` / `trigger` / `poly_trigger`),
outputs, and parameters (with type, range, and default). Shape-varying
modules (e.g. `Sum`, `Delay`, `StereoMixer`) print both a `channels = 1`
and `channels = 2` section so the indexed-port pattern is visible.

Use this when:

- a user asks about a module you are unsure of — grep the output
- you need a parameter's exact name or enum variants
- you are choosing between mono / poly variants
- you want to confirm whether a module takes a `channels: N` shape arg

Pipe through `grep -A N "^## ModuleName"` to isolate a single module.

## Checklist before handing a patch to the user

1. Does every output eventually reach `AudioOut`?
2. Are `PolyToMono` outputs attenuated before the output or limiter?
3. Do ADSR modules receive both `trigger` and `gate`?
4. Does each voice template expose a coherent interface
   (`voct`, `trigger`, `gate`, optionally `velocity`, `audio`)?
5. Are parameters meaningful names — not magic numbers littering the
   patch block?
6. For sequencer patches: do the `song` track names match
   `MasterSequencer`'s `channels`, and do `PatternPlayer`s consume the
   right `seq.clock[...]`?
7. Check against a similar example in this directory if the user's
   request resembles one (poly synth → `poly_synth_layered.patches`;
   drums → `drum_machine.patches`; multi-voice tracker →
   `tracker_three_voices.patches`; modular effects → `pad.patches`,
   `song1/`).
