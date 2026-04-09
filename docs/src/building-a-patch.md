# Building a patch

A patch is a set of module instances and the cables between them. This chapter builds up the concepts one at a time, starting from a single module.

## Modules

A module declaration gives the instance a name and a type:

```patches
module osc : Osc
```

This creates an oscillator named `osc`. The name is how you refer to it when making connections; the type determines what it does and what ports it has.

### Parameters

Most modules accept parameters that configure their behaviour. Parameters are written in braces after the type:

```patches
module osc : Osc { frequency: 440Hz }
module env : Adsr { attack: 0.01, decay: 0.1, sustain: 0.8, release: 0.3 }
```

Parameter values have types. The DSL supports several literal forms:

| Syntax | Meaning |
|--------|---------|
| `440.0` | float |
| `2` | integer |
| `440Hz`, `2.5kHz` | frequency — converted to V/oct internally |
| `-6dB` | decibels — converted to linear amplitude |
| `C4`, `Bb2` | note name — converted to V/oct |
| `true` / `false` | boolean |
| `linear` | bare string (quotes optional) |

Duration parameters like `attack` and `release` are bare floats in seconds — there is no time-unit suffix.

### Shape arguments

Some modules have a variable number of ports. Shape arguments control this and are written in parentheses before the parameter braces:

```patches
module mix : Sum(channels: 3)
module dly : Delay(channels: 4)
```

`Sum(channels: 3)` creates a mixer with three indexed inputs: `mix.in[0]`, `mix.in[1]`, `mix.in[2]`.

## Ports and connections

Modules have named input and output ports. A connection (cable) carries a signal from one output to one input:

```patches
osc.sine -> out.in_left
```

This reads: take the `sine` output of `osc` and feed it to the `in_left` input of `out`.

An output can drive multiple inputs (fan-out):

```patches
osc.sine -> out.in_left
osc.sine -> out.in_right
```

But each input accepts exactly one cable. To combine signals, use a mixer:

```patches
module mix : Sum(channels: 2)
osc_a.sine -> mix.in[0]
osc_b.sine -> mix.in[1]
mix.out    -> vca.in
```

### Scaled connections

A cable can carry a scale factor that multiplies the signal:

```patches
osc.sine -[0.5]-> out.in_left     # attenuate by half
lfo.sine -[-1.0]-> osc.phase_mod  # invert
mod.out  -[0.03]-> osc.phase_mod  # light FM
```

This is useful for mixing at different levels, attenuating modulation depth, or inverting a signal. The scale is applied at the cable level.

### Indexed ports

Modules with multi-port shape arguments use bracket notation:

```patches
module mix : StereoMixer(channels: 4) { level[0]: 0.8, pan[0]: -0.5 }

osc.out   -> mix.in_left[0]
osc.out   -> mix.in_right[0]
noise.out -> mix.in_left[1]
```

Indexed parameters can also be grouped with at-block syntax:

```patches
module dly : Delay(channels: 2) {
    @0 { delay_ms: 250, feedback: 0.4 },
    @1 { delay_ms: 375, feedback: 0.3 }
}
```

## Mono and poly

Cables come in two kinds: **mono** (one sample per tick) and **poly** (one sample per voice per tick, typically 16 voices). A module's port kind is fixed by its type — `Osc` has mono ports, `PolyOsc` has poly ports.

You cannot connect a mono output to a poly input or vice versa. Use `MonoToPoly` to broadcast a mono signal to all voices, and `PolyToMono` to sum voices down to mono.

## Cable delay

All cables have a one-sample delay. A module's `process` call sees the values that other modules wrote on the *previous* tick. This means modules can run in any order without affecting the result, and feedback connections are well-defined — they simply carry the previous tick's value.

## A complete example

Two detuned oscillators mixed and sent to stereo output:

```patches
patch {
    module osc_a : Osc { frequency: 220Hz }
    module osc_b : Osc { frequency: 221Hz }
    module mix   : Sum(channels: 2)
    module out   : AudioOut

    osc_a.sawtooth -> mix.in[0]
    osc_b.sawtooth -> mix.in[1]

    mix.out -[0.3]-> out.in_left
    mix.out -[0.3]-> out.in_right
}
```

The 1 Hz detuning produces a slow beating effect. The `-[0.3]->` scaling keeps the output at a comfortable level. Save this, run it, then try changing one frequency or swapping `sawtooth` for `square` while it plays.
