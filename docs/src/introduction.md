# Introduction

Patches is a modular audio synthesiser you program with a text file. You describe a graph of modules — oscillators, filters, envelopes, effects — and how they connect. The engine runs the graph in real time. When you edit the file and save, the engine swaps in the new graph without stopping the audio or resetting module state. This makes it usable as a live-coding instrument: you perform by editing.

## Building

```bash
git clone <repo-url>
cd patches
cargo build
```

## A first patch

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

A 440 Hz sine tone. Change the frequency to `220Hz`, save, and the pitch drops — no click, no restart. Change `osc.sine` to `osc.sawtooth` for a brighter timbre. Press Ctrl-C to stop.

For MIDI-driven patches, connect your controller before starting the player. `MidiIn` and `PolyMidiIn` use the first available MIDI port.
