# Installation & quickstart

## Prerequisites

- Rust toolchain (stable, 1.75 or later recommended)
- A working audio output device

For MIDI input, connect your device before starting the player. The `PolyMidiIn`
and `MidiIn` modules use the first available MIDI port.

## Building

```bash
git clone <repo-url>
cd patches
cargo build
```

All crates build together. No additional system libraries are required beyond
those pulled in by CPAL (the audio backend).

## Running a patch

```bash
cargo run -p patches-player -- examples/fm_synth.patches
```

The player prints the modules it has loaded and then sits waiting for audio.
Edit and save the `.patches` file to hot-reload.

Press Ctrl-C to stop.

## Available examples

| File | Description |
|---|---|
| `examples/fm_synth.patches` | FM synthesiser driven by MIDI |
| `examples/poly_synth.patches` | Polyphonic subtractive synth |
| `examples/poly_synth_layered.patches` | Layered voices via templates |
| `examples/radigue_drone.patches` | Sine drone with ring modulation |
