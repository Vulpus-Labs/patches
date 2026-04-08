# Introduction

Patches is a Rust system for building modular audio synthesisers using a plain-text
DSL. Patches can be edited and saved while audio is playing; the engine re-plans the
graph and swaps it in without stopping the audio stream or resetting module state
(oscillator phase, filter history, envelope position, etc.).

The intended use case is **live-coding performance**: you write and modify a patch
in your text editor while the sound plays, and changes take effect immediately on
save.

## Key concepts

**Modules** are the building blocks — oscillators, filters, envelopes, mixers, and
so on. Each module has a fixed set of named input and output ports.

**Cables** connect an output port on one module to an input port on another.
Connections can optionally carry a scale factor, so a single connection can also
attenuate or invert a signal.

**Patches** describe a set of module instances and the cables between them, written
in the `.patches` DSL.

**Templates** are reusable sub-graphs with typed parameters. They are expanded by
the DSL compiler before the graph is built, so they carry no runtime cost.

**Hot-reload** is the ability to change the patch file and have the running engine
adopt the new graph. Modules whose identity and type are unchanged keep their
internal state; only structurally new modules are freshly initialised.

## A minimal patch

```patches
patch {
    module osc : Osc { frequency: 440Hz }
    module out : AudioOut

    osc.sine -> out.in_left
    osc.sine -> out.in_right
}
```

Save this as `hello.patches` and run:

```bash
cargo run -p patches-player -- hello.patches
```

You should hear a 440 Hz sine tone. Edit the frequency and save — the pitch
changes without a click or reset.
