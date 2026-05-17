# Mental model

A patch is a directed graph. Boxes are **modules**; arrows are **cables**. A module reads from its inputs, computes, and writes to its outputs once per audio sample. Cables carry the output of one module to one or more inputs of others.

That is the whole model. Everything else — the DSL syntax, the polyphony rules, the live reload behaviour — is a way of building and editing this graph.

## Modules

A module is a unit of computation with a fixed set of named inputs and outputs. Examples:

- An **oscillator** has no inputs (or some control inputs) and produces an audio output.
- A **filter** has an audio input and one or more control inputs (cutoff, resonance), and produces a filtered audio output.
- An **envelope** has a gate input and a trigger input, and produces a control signal that rises and falls.
- An **output module** has an audio input and writes to the soundcard.

The set of ports is defined by the module's *type*. An instance of the type carries its own state — an oscillator's phase, a filter's history, an envelope's position. State is private to the instance.

## Ports and signal kinds

Each port has a name (`in`, `out`, `cutoff`, `gate`, `voct`) and a fixed *kind*. The kind decides what can connect to it.

- **Audio** — a stream of audio samples. Centred on zero, typically in the range −1 to +1.
- **Control voltage (CV)** — a stream of slow-changing modulation values. The convention is **V/oct**: 1 unit = 1 octave. Pitch and many modulation signals use this.
- **Gate** — a level held high (≥ 0.5) or low (< 0.5). Tells an envelope a note is being held.
- **Trigger** — a sub-sample-accurate event encoding. `0.0` means "no event on this sample"; a value in `(0.0, 1.0]` is the fractional position of an event within the sample. Tells an envelope to restart.
- **Stereo** — a pair `(L, R)` of audio samples on one cable. Symmetric stereo modules use a single stereo port rather than two mono ports.

Audio, CV, and gate all ride on the same mono *Audio* cable layout — the engine doesn't distinguish them, and the difference is only how a module *interprets* what it reads. You can deliberately abuse this: feed an audio signal into a CV input for aggressive modulation, or use an envelope's output as audio for a one-shot percussive sample.

Trigger is a different cable layout. It encodes sub-sample event positions, not levels, and the connection validator rejects audio↔trigger wiring in either direction — use the appropriate event source or a conversion module. Poly cables have further distinct layouts for transport frames and packed MIDI events; same rule applies, no implicit coercion across layouts.

## Mono and poly

Cables come in two flavours: **mono** (one sample per audio tick) and **poly** (one sample per voice per tick, typically 16 voices). A module type is either mono or poly throughout — there is `Osc` (mono) and `PolyOsc` (polyphonic), `Adsr` and `PolyAdsr`, and so on.

A mono cable cannot connect to a poly input or vice versa. Use a `MonoToPoly` to broadcast one mono signal to every voice, and `PolyToMono` to sum voices down to a single mono signal.

Stereo and mono interact more permissively: a mono source feeding a stereo input is silently broadcast (`L = R = source`). The reverse — stereo into mono — is an error; if you want it, route through a `StereoSplitter`.

## Cables

A cable connects exactly one output to exactly one input. To split a signal to several destinations, draw several cables from the same output (this is free — no mixing node required). To combine signals into one input, route them through a mixer module — the destination only accepts one cable.

A cable can carry a **scale factor** that multiplies the signal as it passes:

```
osc.sine -[0.3]-> out.in
```

This is a per-cable gain; it is useful for trimming levels, scaling modulation depth, or inverting (`-1.0`).

### Cable delay

Cables in cycle-bearing regions of the graph impose a **one-sample delay**: a module reads what its neighbours wrote on the *previous* tick. This is what makes feedback loops well-defined — a feedback cable carries the previous tick's value, and modules in a cycle can be evaluated in any order.

In acyclic (feedback-free) regions, the planner fuses the chain so consumers read producers' current-tick output. This is invisible to the patch author — the audio result matches the conceptual model — but it removes the one-sample delay that would otherwise accumulate audibly across a long signal chain.

## A patch is a file

A patch is described in a `.patches` file: a single text document that declares modules, sets their parameters, and draws cables between them. There is no project file, no binary patch format. You edit the file; the engine runs the graph the file describes.

When you save, the engine builds the new graph and swaps it in without stopping the audio. Module instances whose name and type are unchanged carry their state across the swap. This is the REPL property — small edits sound like parameter tweaks, not restarts.

## Visualising the graph

The graph can be rendered as an SVG diagram with `patches-svg` (covered in *Visualising patches*). Reading the diagram is a useful way to understand a patch you didn't write; writing one rarely needs the picture.
