# Introduction

Patches is a modular audio instrument with a text-based patch language. You describe a graph of modules — oscillators, filters, envelopes, sequencers, effects — and how they connect; the engine runs the graph in real time. Save the file and Patches swaps in the new graph without interrupting audio or resetting module state, so editing the patch is itself the development cycle.

## Why Patches?

Most software modular systems present a graphical reproduction of a Eurorack-style hardware modular setup. You drag patch cables between ports, turn knobs, move sliders. This is satisfying and creatively rewarding, but it isn't the only way to work.

Patches sets the visual interface aside and concentrates on two things: a clear language for describing patches, and a fast, real-time-stable engine for running them. The language is supported by a full Language Server Protocol implementation, so any compatible editor can offer autocomplete, inline hints, and navigation between definitions.

Working in text makes some abstractions natural that would be awkward in a visual environment. Two examples:

* Templates. Define a sub-patch once with parameters, then instantiate it multiple times with different values.
* Multi-channel modules. A mixing desk or multi-tap delay can be declared with whatever number of channels you need, each with its own parameters, inputs, and outputs.

## Three ways to run a patch

Patches has no single intended workflow. The same `.patches` file works in any of these contexts:

**As a CLAP plugin in a DAW.** Load the `Patches` plugin in REAPER, Bitwig, or any CLAP host. Point it at a patch file, drive it from MIDI tracks, automate its parameters, save with the project. Use this when you want a custom synth voice inside a larger production.

**As a standalone instrument driven by external MIDI.** Run `patch_player hello.patches`, connect a controller, play. Use this when you want a hardware-style instrument with no DAW in the loop.

**As a self-contained piece driven by built-in sequencing.** A patch can include the tracker, step sequencer, and clock modules — its own note source. Run it in the player and the piece plays itself. Use this when the patch *is* the composition.

The DSL, module set, and audio engine are identical across all three. Pick the surface that fits the task.

## A first patch

```patches
patch {
    module osc : Osc { frequency: 440Hz }
    module out : AudioOut

    osc.sine -> out.in
}
```

Save as `hello.patches` and run:

```bash
patch_player hello.patches
```

A 440 Hz sine tone. Change the frequency to `220Hz`, save, and the pitch drops — no click, no restart. Change `osc.sine` to `osc.sawtooth` for a brighter timbre. Ctrl-C to stop.

## What's next

- **Mental model** — how patches are built from modules, ports, cables, and signal kinds. Read this before the DSL chapters; it is the conceptual foundation everything else assumes.
- **Installation** — getting the player, CLAP plugin, and VS Code extension onto your machine.
- **Basic operation** — running existing patches in each of the three workflows.
- **The DSL** — the patch language in detail.
- **Patch authoring** — building real instruments and pieces.
- **Module reference** — every built-in module, its ports, and its parameters.
- **Extending Patches** — writing modules in Rust, or as native plugins via the ABI.
- **Internals** — the audio engine, CLAP plugin, and language server.
