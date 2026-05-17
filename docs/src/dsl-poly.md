# Poly and voice allocation

A *poly* cable carries one sample per voice per audio tick, where
"voice" is one of a fixed-size pool. Patches uses a pool size of
**16 voices** in every shipping host (player, CLAP plugin,
integration tests). A poly cable's data is laid out as 16 parallel
mono streams.

This chapter covers how poly works at the patch level: how to
introduce voices, how MIDI gets mapped into them, what happens when
the pool fills up, and how to bridge between mono and poly.

## Poly module types

Most module types come in mono-only flavour. Poly variants exist for
the building blocks of a polyphonic voice:

| Mono           | Poly                  |
| -------------- | --------------------- |
| `Osc`          | `PolyOsc`             |
| `Adsr`         | `PolyAdsr`            |
| `Vca`          | `PolyVca`             |
| `Svf`          | `PolySvf`             |
| `Glide`        | `PolyGlide`           |
| `MidiToCv`     | `PolyMidiToCv`        |

A poly module's ports are poly throughout: `PolyOsc.voct` is a poly
CV input that takes one V/oct value per voice, and its `sawtooth`
output is a poly audio stream of 16 sawtooths. An *entire* voice —
oscillator, envelope, filter, VCA — is wired in poly and produces
16 parallel voice outputs, which are then mixed down to a mono or
stereo signal for the master bus.

## Voice allocation: PolyMidiToCv

The standard way to introduce poly is with `PolyMidiToCv`. It
receives MIDI note-on / note-off events and assigns each note to a
voice slot, exposing per-voice `voct`, `gate`, `trigger`, and
`velocity` poly outputs.

```patches
module kbd : PolyMidiToCv
module osc : PolyOsc
module env : PolyAdsr {
    attack: 0.005, decay: 0.1, sustain: 0.7, release: 0.3
}
module vca : PolyVca
module mix : PolyToMono
module out : AudioOut

kbd.voct    -> osc.voct
kbd.gate    -> env.gate
kbd.trigger -> env.trigger

osc.sine -> vca.in
env.out  -> vca.cv
vca.out  -> mix.in
mix.out -[0.2]-> out.in
```

The signal chain is wholly poly from `kbd` through `vca`; `PolyToMono`
sums the 16 voices into one mono stream before output.

### Allocation policy

When a new note arrives:

1. **A free voice exists.** The note takes the first free slot.
2. **All voices are active.** The most-recently-allocated voice is
   *stolen* (LIFO — last in, first out): its gate goes low briefly,
   then the new note takes its slot.

This is fast, predictable, and avoids the "oldest voice dropping
mid-release" surprise of FIFO stealing. The trade-off: rapidly
restruck notes can chain-steal each other if you exceed 16
simultaneous voices.

Note-off releases the matching voice, gating its envelope down. The
voice slot becomes free for reuse after the envelope's release
phase finishes (or immediately, if a new note steals it).

### Velocity

`PolyMidiToCv.velocity` carries the per-voice velocity as a CV
signal (`0` to `1`). Route it into a `PolyVca`'s CV input to
velocity-shape the envelope:

```patches
module vel_vca : PolyVca
env.out      -> vca.cv
vca.out      -> vel_vca.in
kbd.velocity -> vel_vca.cv
vel_vca.out  -> mix.in
```

## Bridges: MonoToPoly / PolyToMono

Mono and poly cables cannot connect directly. Two bridge modules
handle the boundary.

### MonoToPoly — broadcast

`MonoToPoly` copies a mono signal to every voice. Useful when a
single LFO or envelope modulates all voices identically:

```patches
module lfo : Lfo { rate: 0.5 }
module mtp : MonoToPoly

lfo.sine -> mtp.in
mtp.out  -> poly_osc.frequency_cv   # All voices receive the same LFO
```

After `MonoToPoly`, every poly voice slot carries the source's
sample value.

### PolyToMono — sum

`PolyToMono` sums the active voices into a single mono stream. This
is the standard way to take a poly voice chain to the master bus:

```patches
module ptm : PolyToMono
poly_vca.out -> ptm.in
ptm.out      -> reverb.in
```

`PolyToMono` only sums the *active* voices. Inactive slots
contribute zero — there's no per-voice CPU overhead for unused
voices in the mix-down, though every poly module still runs all 16
slots per tick.

## Where poly costs come from

Poly modules run all 16 voice slots every tick. A `PolyOsc` is, at
runtime, 16 oscillator instances running in parallel — but they
share state allocation, control-rate updates, and DSP kernel calls,
so the per-voice overhead is much lower than 16 separate `Osc`
instances would be.

In practice:

- Poly voice chains scale linearly with module count in the chain,
  not with active voice count.
- Mono modules between poly nodes (e.g. a global reverb fed by
  `PolyToMono`) cost the same regardless of how many voices are
  playing.
- Effects you want per-voice (chorus, drive, voice-specific filter)
  go *inside* the poly chain; effects on the global sound go
  *after* the mix-down.

## Mixing mono and poly

There's no rule against using a mono `MidiToCv` for a lead voice and
a `PolyMidiToCv` for a pad in the same patch — the engine doesn't
care. The same physical MIDI input feeds both modules; each does its
own allocation.

```patches
module lead : MidiToCv         # mono — monophonic lead
module pad  : PolyMidiToCv     # 16-voice pad
```

This is the typical structure for a hybrid synth: a mono lead voice
with its own dedicated note source, plus a poly pad voice running in
parallel. Both modules see the same MIDI traffic.

## What's next

`PolyMidiToCv` is the most common source of poly traffic, but other
modules also produce poly streams — sequencers with multiple voice
outputs, drum-rack patterns, modulation matrices. See the
[Sequencers & clocks](modules/sequencers.md) and
[MIDI](modules/midi.md) reference pages for the full set.
