# Signal kinds and cable rules

Every port has a *kind*: what flavour of signal it carries. The kind
decides which cables can connect to it. The five kinds —
**Audio**, **CV**, **Gate**, **Trigger**, **Stereo** — were
introduced in [Mental model](mental-model.md). This chapter lays out
the rules the planner enforces and the conversions that happen for
you.

## The five kinds, quickly recapped

| Kind        | Sample stream                              | Typical use                 |
| ----------- | ------------------------------------------ | --------------------------- |
| **Audio**   | Float samples, roughly `[-1, +1]`.         | Sound. Anything you'd hear. |
| **CV**      | Float samples, slow. V/oct convention.     | Pitch, modulation.          |
| **Gate**    | `1.0` (held) or `0.0` (released).          | Note hold.                  |
| **Trigger** | Single-sample `1.0` pulse.                 | Note start.                 |
| **Stereo**  | One cable carrying `(L, R)` pairs.         | Stereo audio.               |

Underneath, the engine just streams `f32` samples — there is no
runtime tag distinguishing audio from gate. The kind is a *typing*
discipline. Patches modules declare their ports' kinds and the
planner refuses connections that violate the rules.

The freedom of "samples are samples" is intentional: you can
deliberately feed audio into a CV input for aggressive modulation,
or run an envelope's output through `AudioOut` as a one-shot
percussion sample. The type system lets the planner catch the
mistakes worth catching while leaving the creative misuse paths
open.

## Mono and poly are orthogonal

A cable is also either **mono** (one sample per audio tick) or
**poly** (one sample per voice per tick — typically 16 voices). This
is independent of the kind:

- `Osc` (mono) has mono audio outputs.
- `PolyOsc` (polyphonic) has poly audio outputs.
- Similarly `Adsr` (mono CV out) and `PolyAdsr` (poly CV out).

A module instance is wholly mono or wholly poly; there is no
"mostly mono with one poly port" mixed case. The poly counterpart
of a mono module type is a separate type, usually prefixed `Poly`.

## Cable rules

The planner enforces the following rules at compile time:

### Identical kind → free

Audio→Audio, CV→CV, Gate→Gate, Trigger→Trigger, Stereo→Stereo all
connect without ceremony.

```patches
osc.sine    -> filt.in       # Audio → Audio
lfo.sine    -> osc.voct      # CV    → CV
midi.gate   -> env.gate      # Gate  → Gate
midi.trigger -> env.trigger  # Trigger → Trigger
```

### Audio ↔ CV / Gate / Trigger → free

Because they are all `f32` streams under the hood, mono Audio /
CV / Gate / Trigger can be cross-connected. The planner does not
flag this — it's the deliberate-misuse path.

This is a feature: routing an envelope's output as audio gives a
one-shot percussion sample; routing an audio signal into a CV input
gives audio-rate modulation.

### Mono Audio → Stereo input → silent broadcast

A mono audio output feeding a stereo audio input is *silently
broadcast*: `L = R = source`. No synthetic splitter node, no warning.

```patches
osc.sine -> dly.in           # dly.in is stereo; sine is mono
                             # → both channels receive sine
```

This removes the need for explicit broadcasters at every
mono-to-stereo boundary.

### Stereo → mono input → rejected

The reverse is *not* permitted. Compiling fails with
`CableKindMismatch`. If you want to mix down, route through
`PolyToMono` for poly, or `StereoSplitter` / `StereoJoiner` for
stereo when you want both halves explicitly.

```patches
dly.out -> mono_filt.in      # ERROR: stereo → mono
```

Use `StereoSplitter` first if the split is intentional:

```patches
module split : StereoSplitter
dly.out      -> split.in
split.left   -> mono_filt.in
```

### Mono ↔ Poly → bridge module required

Mono and poly are not auto-convertible. Use `MonoToPoly` to
broadcast a mono signal to every voice:

```patches
module mtp : MonoToPoly
lfo.sine -> mtp.in
mtp.out  -> poly_osc.voct    # poly_osc.voct now sees the LFO on all voices
```

And `PolyToMono` to sum poly voices into a single mono stream:

```patches
module ptm : PolyToMono
poly_vca.out -> ptm.in
ptm.out      -> reverb.in
```

There is no automatic broadcast or sum; the type mismatch must be
handled by an explicit module so it appears in the patch.

## Fan-out and fan-in

Two separate rules, distinct from kind rules:

- **Fan-out is free.** An output port can drive any number of
  inputs. Draw multiple cables from it; no "split" module needed.
- **Fan-in needs a mixer.** An input port accepts exactly *one*
  cable. To combine signals into one input, route them through a
  `Sum`, `Mixer`, `StereoMixer`, or similar.

```patches
# Fan-out: one LFO modulates three destinations
lfo.sine -> osc.frequency_cv
lfo.sine -> filt.cutoff_cv
lfo.sine -> vca.cv

# Fan-in: three sources sum into one filter — auto-summed,
# no explicit Sum module needed
osc_a.sine, osc_b.sine, osc_c.sine -> filt.in
```

## Cable delay

This is a subtle one and matters for understanding when feedback is
well-defined.

In a cycle-bearing region of the graph, every cable carries a
**one-sample delay**: a module's `process` call sees what its
neighbours wrote on the *previous* tick. This is what makes feedback
loops have a well-defined value at every tick — a feedback cable is
simply yesterday's sample.

In acyclic (feedback-free) regions, the planner [fuses the
chain](https://en.wikipedia.org/wiki/Topological_sorting) so
consumers read producers' *current-tick* output. This is invisible
to you — the audio matches the conceptual model — but it removes
the one-sample-per-cable lag that would otherwise accumulate
audibly across a long signal chain.

You don't need to think about this when writing a patch. The cable
delay matters only when you're building feedback loops where the
one-sample latency is a feature (a delay line's feedback path, a
self-oscillating filter, a phase-modulation loop).

## Summary

| Source kind   | Destination kind    | Allowed?                 |
| ------------- | ------------------- | ------------------------ |
| Audio (mono)  | Audio (mono)        | yes                      |
| Audio (mono)  | CV / Gate / Trigger | yes (intentional misuse) |
| Audio (mono)  | Stereo              | yes — `L = R = source`   |
| Stereo        | Stereo              | yes                      |
| Stereo        | Mono Audio          | **no** — splitter needed |
| Mono          | Poly                | **no** — `MonoToPoly`    |
| Poly          | Mono                | **no** — `PolyToMono`    |
| Poly Audio    | Poly Audio          | yes                      |

If a connection is rejected, the diagnostic will name the source
kind, the destination kind, and the rule that fails. Read it
literally — it tells you which bridge module to insert.
