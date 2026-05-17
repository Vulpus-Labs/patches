# DSL basics

This chapter introduces the syntax for declaring modules, setting
their parameters, and drawing cables between them. It assumes the
concepts from [Mental model](mental-model.md) — modules, ports,
cables, signal kinds — and shows how to write them down.

For the complete grammar with every form, see the [DSL
reference](dsl-reference.md). This chapter is the on-ramp; that is
the lookup table.

## The patch block

Every `.patches` file ends with exactly one `patch { ... }` block. It
contains module declarations, connections, and (optionally)
pattern/song/template/host-control blocks. Comments run from `#` to
end of line; whitespace and blank lines don't matter.

```patches
patch {
    module osc : Osc { frequency: 440Hz }
    module out : AudioOut

    osc.sine -> out.in
}
```

A 440 Hz sine to the soundcard. Save and run.

## Modules

A module declaration gives an instance a name and a type:

```patches
module osc : Osc
```

- **Name** — your handle for the instance (`osc`). Used in
  connections, and used for identity-matching across hot-reloads: a
  module whose name and type are unchanged carries its state
  across a reload.
- **Type** — the module type as registered in the module registry
  (`Osc`, `PolyAdsr`, `Svf`, `StereoMixer`). Determines what ports
  the instance has.

The set of available types is the [module reference](modules/README.md).

### Parameters

Most modules take parameters in braces after the type:

```patches
module osc : Osc { frequency: 440Hz }
module env : Adsr { attack: 0.005, decay: 0.1, sustain: 0.7, release: 0.3 }
```

Parameter values support several literal forms:

| Syntax             | Meaning                                            |
| ------------------ | -------------------------------------------------- |
| `440.0`, `-0.5`    | Float.                                             |
| `2`, `-1`          | Integer.                                           |
| `440Hz`, `2.5kHz`  | Frequency. Converted to V/oct internally.          |
| `-6dB`             | Decibels. Converted to linear amplitude.           |
| `7s`               | Semitones. Converted to V/oct (`s` = *semitone*).  |
| `100c`             | Cents. Converted to V/oct.                         |
| `C4`, `Bb2`, `F#5` | Note name. Converted to V/oct.                     |
| `true`, `false`    | Boolean.                                           |
| `plate`, `linear`  | Bare identifier (used by enum parameters).         |
| `"some string"`    | Quoted string.                                     |

There is **no** seconds suffix. Time parameters take a bare float
(`attack: 0.005` is 5 ms). The `s` after a number means *semitones*,
not seconds — `7s` is "seven semitones above C0", which is G0. The
`Hz`/`kHz`/`dB`/`s`/`c` suffixes are parsed as part of the literal
and converted at compile time, so `frequency: 440Hz` and
`frequency: 440.0` are not equivalent — the first means 440 hertz,
the second means 440 V/oct (which is nonsense). Use the right unit.

### Shape arguments

Some modules have a variable number of ports. The shape is set in
parentheses *before* the parameter braces. **Prefer a named alias
list** — it documents what each channel is for and lets you address
the indexed ports by name everywhere they appear:

```patches
module mix : StereoMixer([drums, bass, lead]) {
    @drums: { level: 0.8, pan:  0.0 },
    @bass:  { level: 0.7, pan: -0.2 },
    @lead:  { level: 0.6, pan:  0.3 }
}
module dly : StereoDelay([short, long]) { delay_ms: 750, feedback: 0.4 }
```

The aliases become channel names: `mix.in[drums]`, `mix.in[bass]`,
`mix.in[lead]`. A bare count (`StereoMixer(3)`) or the named-count
form (`StereoMixer(channels: 3)`) work too — they fall back to
numeric indices (`mix.in[0]`, `mix.in[1]`, …) — but the alias form
reads better and survives reordering.

### Indexed parameters

Per-channel parameters are written with a bracket index — using the
alias declared on the shape:

```patches
module mix : StereoMixer([drums, bass, lead]) {
    level[drums]: 0.8, pan[drums]:  0.0,
    level[bass]:  0.7, pan[bass]:  -0.2,
    level[lead]:  0.6, pan[lead]:   0.3
}
```

Or, more concisely, with the **at-block** form that groups all
fields for one channel:

```patches
module mix : StereoMixer([drums, bass, lead]) {
    @drums: { level: 0.8, pan:  0.0 },
    @bass:  { level: 0.7, pan: -0.2, send_a: 0.3 },
    @lead:  { level: 0.6, pan:  0.3, send_a: 0.3 }
}
```

Numeric at-blocks (`@0: { ... }`) work when you declared the shape
with a bare count.

The colon after `@N` is optional. Channels not listed take the
parameter defaults defined by the module.

## Connections

A connection (cable) carries a signal from one output port to one
input port:

```patches
osc.sine -> filt.in
filt.lowpass -> vca.in
env.out -> vca.cv
vca.out -> out.in
```

`module.port` on each side; an arrow `->` between them.

**Fan-out is free.** An output can drive many inputs without an
explicit "split" module — draw multiple cables from the same output:

```patches
lfo.sine -> osc.frequency_cv
lfo.sine -> filt.cutoff_cv
```

**Fan-in is summed automatically.** List multiple sources before the
arrow and the expander synthesises a `Sum` (or `PolySum` /
`StereoSum`, depending on cable kind) under the hood:

```patches
osc_a.sine, osc_b.sine -> out.in
```

When you want explicit level control per channel — sends, mute/solo,
panning — declare a `Mixer` or `StereoMixer` and route into its
indexed inputs instead.

### Scaled cables

A cable can multiply its signal by a constant. Write the scale in
square brackets between the dashes:

```patches
osc.sine    -[0.3]-> out.in        # attenuate
lfo.sine    -[-1.0]-> osc.voct      # invert
mod.out     -[0.03]-> osc.phase_mod # light FM
mix.out  -[-12dB]-> out.in          # attenuate by 12 dB
```

The scale can be a plain number or a unit literal (`dB`, `Hz`, etc.,
where it makes sense for the destination).

### Range-mapped cables

For modulation depth that depends on the destination's natural unit,
use `uni(lo, hi)` (maps a `[0, 1]` source into `[lo, hi]`) or
`bi(lo, hi)` (maps a `[-1, 1]` source into `[lo, hi]`):

```patches
lfo.sine -[bi(-1Hz, 1Hz)]-> osc.frequency_cv
env.out  -[uni(0Hz, 1kHz)]-> filt.cutoff_cv
```

See the [DSL reference](dsl-reference.md) for the full semantics and
the cases where range-mapping matters.

### Indexed ports

Connect to a specific channel by its alias (declared on the
module's shape) or by numeric index. Aliases are the better default —
they document what each cable is for and don't break when channels
are added or reordered.

```patches
module mix : StereoMixer([bass, lead1, lead2])

bass_v.audio  -> mix.in[bass]
lead1_v.audio -> mix.in[lead1]
lead2_v.audio -> mix.in[lead2]
```

Numeric indices still work for modules declared with a bare count
(`StereoMixer(3)` gives you `mix.in[0]`, `mix.in[1]`, `mix.in[2]`)
and are fine for throwaway sketches. Reach for aliases as soon as
the patch has more than two channels or you find yourself counting.

## A complete example

Detuned sawtooth pair through a filter to stereo out:

```patches
patch {
    module osc_a : Osc { frequency: 220Hz }
    module osc_b : Osc { frequency: 221Hz }
    module filt  : Lowpass { cutoff: 1.5kHz }
    module out   : AudioOut

    osc_a.sawtooth, osc_b.sawtooth -> filt.in
    filt.out -[0.3]-> out.in
}
```

Run it, then change `221Hz` to `222Hz` and save — the beating slows
without restarting the audio.

## What's next

- [Signal kinds and cable rules](dsl-signal-kinds.md) — what
  connects to what, and why.
- [Templates and abstraction](dsl-templates.md) — building reusable
  named subgraphs.
- [Poly and voice allocation](dsl-poly.md) — polyphony, voice
  counts, MIDI-to-CV.
- [DSL reference](dsl-reference.md) — every syntax form, for lookup.
