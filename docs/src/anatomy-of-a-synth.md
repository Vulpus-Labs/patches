# Anatomy of a synth

This chapter walks through `examples/poly_synth.patches` — a 16-voice polyphonic subtractive synthesiser driven by MIDI. It is a good example of how modules compose into a real instrument, and of the kinds of edits you might make while performing.

## The full patch

```patches
patch {
    module kbd       : PolyMidiIn
    module vibrato   : Lfo        { rate: 5.0 }
    module pm_spread : MonoToPoly
    module osc       : PolyOsc
    module mix       : PolySum(channels: 2)
    module amp_env   : PolyAdsr   { attack: 0.005, decay: 0.12,  sustain: 0.65, release: 0.4 }
    module vca       : PolyVca
    module filt_env  : PolyAdsr   { attack: 0.2,   decay: 0.25,  sustain: 0.3,  release: 0.4 }
    module filt      : PolyLowpass { cutoff: 600.0Hz, resonance: 0.7, saturate: true }
    module collapse  : PolyToMono
    module out       : AudioOut

    # Vibrato LFO → phase modulation on all voices
    vibrato.sine   -[0.03]-> pm_spread.in
    pm_spread.out         -> osc.phase_mod

    # V/oct: keyboard → oscillator (all voices)
    kbd.voct               -> osc.voct

    # Oscillator mix: sawtooth (body) + square (edge)
    osc.sine       -[0.6]-> mix.in[0]
    osc.square     -[0.4]-> mix.in[1]

    # Amplitude envelope: triggered and gated per voice
    kbd.trigger            -> amp_env.trigger
    kbd.gate               -> amp_env.gate

    # VCA: shaped audio per voice
    mix.out                -> vca.in
    amp_env.out            -> vca.cv

    # Filter envelope: triggered and gated per voice
    kbd.trigger            -> filt_env.trigger
    kbd.gate               -> filt_env.gate

    # Filter cutoff CV: envelope sweeps each voice's cutoff
    filt_env.out   -[0.7]-> filt.voct

    # Filter: VCA output → lowpass → collapse
    vca.out                -> filt.in

    # Collapse all voices to mono
    filt.out               -> collapse.in

    # Stereo output — scale down to avoid clipping with many voices
    collapse.out  -[0.12]-> out.in_left
    collapse.out  -[0.12]-> out.in_right
}
```

## Signal flow

The patch has a classic subtractive architecture: oscillator → mixer → amplifier → filter → output. What makes it interesting is that every stage from the oscillator onwards is polyphonic — each MIDI voice has its own independent signal path — until the very end, where all voices are summed to mono for output.

```
kbd ──voct──→ osc ──sine──→ mix ──→ vca ──→ filt ──→ collapse ──→ out
               ↑  ──square──↗       ↑        ↑
           vibrato            amp_env    filt_env
```

## MIDI input

`PolyMidiIn` listens on the first available MIDI port and allocates incoming notes across 16 voices using LIFO (last-in, first-out) voice stealing. It outputs three poly signals:

- **voct** — pitch as a voltage, following the V/oct convention (MIDI note 0 = 0 V, each semitone adds 1/12 V). This drives the oscillator's pitch directly.
- **gate** — high (1.0) while a key is held, low (0.0) when released. Envelopes use this to know when to enter their release phase.
- **trigger** — a single-sample pulse (1.0) at the moment a note starts. Envelopes use this to restart from the attack phase.

The distinction between gate and trigger matters. The gate tells the envelope *how long* the note is held; the trigger tells it *when* to restart. An envelope that only received a gate would not retrigger if you played the same note twice in quick succession — the gate would stay high.

## Oscillators and mixing

`PolyOsc` generates multiple waveforms simultaneously — sine, sawtooth, square, triangle — all at the pitch set by `voct`. Here we mix two of them:

```patches
osc.sine       -[0.6]-> mix.in[0]
osc.square     -[0.4]-> mix.in[1]
```

The scale factors set the blend: 60% sine, 40% square. `PolySum(channels: 2)` adds its inputs together. Changing these ratios is one of the simplest live tweaks — shift the balance towards square for a harder, more hollow sound, or towards sine for something rounder.

## Amplitude shaping

Without a VCA, every voice would sound continuously. The amplitude envelope gives each note its dynamic shape:

```patches
kbd.trigger -> amp_env.trigger
kbd.gate    -> amp_env.gate
mix.out     -> vca.in
amp_env.out -> vca.cv
```

`PolyAdsr` produces a control signal that rises over the attack time (0.005s — a fast 5ms onset), falls to the sustain level (0.65) over the decay time (0.12s), holds there while the gate is high, then drops to zero over the release time (0.4s) when the key is released.

`PolyVca` multiplies the audio signal by this control voltage. When the envelope output is 0, the voice is silent. When it is 1, the voice is at full volume.

## Filter modulation

The filter has its own envelope with different timing:

```patches
filt_env.out -[0.7]-> filt.voct
```

This second envelope sweeps the lowpass cutoff up from the base frequency (600 Hz) on each note attack, then lets it fall back. The attack is slower (0.2s) and the sustain lower (0.3), creating a filter "pluck" — brightness on the onset that fades to a darker sustain.

The `-[0.7]->` scale controls how far the sweep reaches. At 0.7, it opens the filter substantially. Reducing this to 0.2 would give a subtler, darker sweep; raising it past 1.0 would push the cutoff into very bright territory.

The `resonance: 0.7` parameter adds emphasis at the cutoff frequency — a nasal, vocal quality. With `saturate: true`, the filter soft-clips internally, adding warmth when driven hard.

## Vibrato

A mono LFO provides gentle pitch modulation:

```patches
vibrato.sine   -[0.03]-> pm_spread.in
pm_spread.out         -> osc.phase_mod
```

The LFO runs at 5 Hz. `MonoToPoly` broadcasts its mono output to all 16 voices so every voice gets the same vibrato. The scale of 0.03 keeps it subtle — just enough to add life without obvious pitch wobble. Push it to 0.1 or higher for a more dramatic effect, or drop the rate to 1 Hz for a slow, seasick drift.

Phase modulation (writing to `osc.phase_mod`) is perceptually similar to frequency modulation but better-behaved at extreme depths.

## Output and level management

The final stage collapses all voices to mono and routes to stereo output:

```patches
filt.out      -> collapse.in
collapse.out -[0.12]-> out.in_left
collapse.out -[0.12]-> out.in_right
```

`PolyToMono` sums all 16 voice signals. If several voices are active simultaneously, their amplitudes add up and can easily exceed 1.0. The `-[0.12]->` scaling compensates — it is roughly 1/8, leaving headroom for eight simultaneous voices at full amplitude. In practice you would adjust this by ear: if it clips, reduce the scale; if it is too quiet, raise it.

## Performing with this patch

The parameters are laid out for live editing. Some modifications to try while the patch is running:

**Tighter attack** — change `amp_env`'s attack to `0.001` for a harder, more percussive onset. Or raise it to `0.1` for a pad-like swell.

**Shorter decay, lower sustain** — set `decay: 0.3, sustain: 0.2` on `amp_env` for a plucked, piano-like envelope where the note fades quickly to a quiet sustain.

**Darker timbre** — drop the filter cutoff to `200Hz` and reduce the envelope scale to `-[0.3]->`. The sound becomes muffled with just a hint of brightness on the attack.

**More resonance** — raise `resonance` towards `0.9`. The filter starts to ring, giving a sharp, acidic edge. Combined with a slow filter envelope, this produces the squelchy sound associated with the TB-303.

**Different waveform blend** — change the mix to `osc.sawtooth -[1.0]->` and remove the square entirely for a classic sawtooth lead. Or use `osc.triangle` for a softer, flute-like quality.

**Wider vibrato** — increase the scale on the vibrato connection to `0.1` and slow the rate to `3.0`. This gives a more expressive, vocal-like pitch movement.

Each of these changes takes effect on the next save. The oscillator phases, envelope positions, and filter states all carry over — you hear the change, not a restart.
