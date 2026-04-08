# patches

A Rust system for defining modular audio patches and running them in a real-time
audio engine, with support for hot-reloading patches at runtime. The intended use
case is live-coding performance: the patch graph can be rebuilt and swapped in
without stopping the audio stream or resetting module state (oscillator phase,
filter history, etc.).

```patches
patch {
    module kbd : PolyMidiIn
    module osc : PolyOsc
    module env : PolyAdsr {
        attack: 0.005, decay: 0.1, sustain: 0.7, release: 0.3
    }
    module vca : PolyVca
    module mix : PolyToMono
    module out : AudioOut

    kbd.trigger -> env.trigger
    kbd.gate    -> env.gate
    kbd.voct    -> osc.voct

    osc.sine    -> vca.in
    env.out     -> vca.cv
    vca.out     -> mix.in
    mix.out -[0.2]-> out.in_left
    mix.out -[0.2]-> out.in_right
}
```

Save this as `hello.patches`, connect a MIDI device, then run:

```bash
cargo run -p patches-player -- hello.patches
```

`PolyMidiIn` uses the first available MIDI input port. The device must be
connected before the process starts. While it is running, edit and save
`hello.patches` to hot-reload the patch without interrupting audio.

## Running patches-player

```text
patch_player [options] <path-to-patch.patches>
```

| Option | Description |
| --- | --- |
| `--oversampling <1\|2\|4\|8>` | Oversampling factor (default: 1) |
| `--record <path.wav>` | Record output to WAV file |
| `--output-device <name>` | Use named output device |
| `--input-device <name>` | Open named input device for audio capture |
| `--list-devices` | List available audio devices and exit |
| `--no-stdin` | Run without stdin monitoring (kill process to stop) |

Examples:

```bash
# Play a patch with hot-reload (press Enter to stop):
cargo run -p patches-player -- examples/poly_synth.patches

# Record to WAV:
cargo run -p patches-player -- --record out.wav examples/fm_synth.patches

# 4× oversampling:
cargo run -p patches-player -- --oversampling 4 examples/radigue_drone.patches

# List audio devices:
cargo run -p patches-player -- --list-devices
```

## VS Code extension

The **Patches DSL** extension provides syntax highlighting, diagnostics, hover
info, and go-to-definition for `.patches` files. It bundles the `patches-lsp`
language server.

### Installing from a GitHub release

1. Go to the [Releases](../../releases) page and download the `.vsix`
   file for your platform (e.g. `patches-vscode-darwin-arm64-*.vsix`).
2. Install it:

   ```bash
   code --install-extension patches-vscode-*.vsix
   ```

   Or in VS Code: **Extensions** → **⋯** menu → **Install from VSIX…**

The extension activates automatically for `.patches` files. The LSP server
binary is bundled inside the `.vsix`; no separate installation is needed.

## Goals

- **Patch DSL** — a DSL for describing signal graphs of audio modules
  (connections, scaling, routing, polyphony, and reusable templates).
- **Audio engine** — a real-time processing pipeline that accepts new patch plans
  without allocating or blocking on the audio thread.
- **Live-reload** — stateful module instances survive re-planning; only
  structurally changed parts of the graph are reset.

## Current state

The full `.patches` DSL is implemented. `patches-player` watches its input file
and re-plans the patch (keeping existing modules running) whenever a new version
is saved.

The core engine and a practical set of modules are in place:

- **`patches-dsl`** — PEG parser and template expander for the `.patches` DSL.
  Produces a `FlatPatch` (modules + edges) with no knowledge of concrete module
  types. Supports: module declarations, inline parameters, typed
  template parameters (`float`, `int`, `bool`, `str`) with defaults,
  indexed ports (`in[1]`), arity wildcards (`[*n]`), scaled connections
  (`-[0.5]->`), at-blocks (`@0: { ... }`), unit literals (`440Hz`,
  `-6dB`, `C4`), and `$` boundary ports for templates.
- **`patches-interpreter`** — validates a `FlatPatch` against the module registry
  and constructs a `ModuleGraph`.
- **`Module` trait** with `prepare` (called once on plan activation) and `process`
  (called per sample via `CablePool`, allocation-free).
- **`ModuleGraph`** for building signal graphs with scaled connections.
- **`ExecutionPlan`** produced by a pure `Planner`; uses a flat buffer pool with a
  1-sample cable delay so modules can run in any order.
- **Audio-thread-owned module pool** — module instances and their state survive
  hot-reloads without crossing the thread boundary.
- **Lock-free plan handoff** to the audio thread via an rtrb ring buffer.
- **Off-thread deallocation** — tombstoned modules and evicted plans are dropped on
  a dedicated `"patches-cleanup"` thread, never on the audio thread.
- **Port connectivity notifications** — modules are notified when their ports are
  connected or disconnected via `set_connectivity`.
- **`patch_player` binary** (`patches-player`) — loads a `.patches` patch,
  plays it, and hot-reloads whenever the file changes on disk.
- **Language server** (`patches-lsp`) — diagnostics, hover, and go-to-definition
  for `.patches` files. Bundled into the VS Code extension.

### Modules

| Module | Description |
| --- | --- |
| **Oscillators** | |
| `Osc` | Mono oscillator (sine, triangle, sawtooth, square) with FM, phase mod, drift |
| `PolyOsc` | Polyphonic oscillator (one phase accumulator per voice) |
| `Lfo` | Low-frequency oscillator (sine, triangle, saw, square, random; sync input) |
| **Envelopes** | |
| `Adsr` | Mono ADSR envelope generator |
| `PolyAdsr` | Per-voice ADSR envelope |
| **Filters** | |
| `Lowpass`, `Highpass`, `Bandpass` | Mono resonant biquad filters |
| `PolyLowpass`, `PolyHighpass`, `PolyBandpass` | Per-voice resonant biquad filters |
| `Svf` | State variable filter (simultaneous LP/HP/BP outputs) |
| `PolySvf` | Polyphonic state variable filter |
| **Amplifiers** | |
| `Vca` | Mono voltage-controlled amplifier |
| `PolyVca` | Per-voice VCA |
| **Mixers** | |
| `Sum` | Multi-input mono sum |
| `PolySum` | Multi-input poly sum (per-voice) |
| `Mixer` | N-channel mixer with level, send A/B, mute, solo |
| `StereoMixer` | Stereo mixer with pan |
| `PolyMixer`, `StereoPolyMixer` | Polyphonic mixer variants |
| `PolyToMono` | Collapse poly voices to mono |
| `MonoToPoly` | Broadcast mono to all poly voices |
| **MIDI** | |
| `MidiIn` | Mono MIDI note input (last-note-priority stack) |
| `PolyMidiIn` | Polyphonic MIDI input with LIFO voice stealing |
| `MidiCC` | MIDI CC to bipolar CV converter |
| **Noise** | |
| `Noise` | Four-colour noise (white, pink, brown, red) |
| `PolyNoise` | Polyphonic four-colour noise |
| **Delays & reverb** | |
| `Delay` | Mono multi-tap delay (4 s, per-tap feedback/tone/drive) |
| `StereoDelay` | Stereo multi-tap delay with pan and pingpong |
| `FdnReverb` | 8-line FDN reverb (plate/room/chamber/hall/cathedral) |
| `ConvReverb`, `StereoConvReverb` | Convolution reverb (built-in IRs or file) |
| **Dynamics** | |
| `Limiter` | Lookahead peak limiter with inter-sample peak detection |
| **Effects** | |
| `RingMod` | Analog ring modulator (Parker diode-bridge model) |
| `PitchShift` | Spectral pitch shifter (WOLA phase vocoder, optional formant preservation) |
| **Sequencers & clocks** | |
| `Clock` | Bar/beat/quaver/semiquaver trigger generator |
| `Seq` | Step sequencer (note strings, gate, trigger) |
| **Utilities** | |
| `Glide` | Portamento / pitch smoothing |
| `Tuner`, `PolyTuner` | Pitch offset (octave + semitones + cents) |
| `Quant`, `PolyQuant` | V/oct quantiser to user-defined note set |
| `Sah`, `PolySah` | Sample and hold |
| **I/O** | |
| `AudioOut` | Stereo audio output (backplane sink) |
| `AudioIn` | Stereo audio input (backplane source) |

## Workspace layout

```text
patches-core/              Core types, traits, and execution plan runtime
patches-dsp/               Pure DSP kernels (filters, delay, noise, ADSR)
patches-dsl/               PEG parser and template expander for .patches DSL
patches-interpreter/       Validates FlatPatch against module registry; builds ModuleGraph
patches-modules/           Audio module implementations
patches-engine/            Patch builder, Planner, PatchEngine, CPAL sound engine
patches-player/            patch_player binary: load, play, hot-reload on change
patches-io/                I/O integration (audio capture, WAV recording)
patches-clap/              CLAP audio plugin host integration
patches-lsp/               Language server for .patches files
patches-ffi/               FFI bindings for native module plugins
patches-ffi-common/        Shared types for FFI plugin interface
patches-profiling/         Profiling utilities
patches-integration-tests/ Cross-crate integration tests (not published)
test-plugins/              Example native plugins (gain, conv-reverb)
patches-vscode/            VS Code extension (syntax highlighting + LSP client)
docs/                      mdBook manual (source in docs/src/)
tickets/                   Work tracking (open / in-progress / closed)
epics/                     Epics grouping related tickets
adr/                       Architecture decision records
examples/                  Example .patches files
```

## Building and running

```bash
cargo build
cargo test
cargo clippy

# Run the player with a patch file (hot-reloads on save):
cargo run -p patches-player -- examples/poly_synth.patches
cargo run -p patches-player -- examples/fm_synth.patches
cargo run -p patches-player -- examples/radigue_drone.patches
```

## Patch format

Patches are written in the `.patches` DSL. A file contains zero or more
template definitions followed by exactly one `patch` block.

### Module declarations

Modules are declared with optional inline parameters; connections use `->` or
`-[scale]->` for scaled cables. Indexed ports use `name[n]` notation.

```patches
patch {
    module osc : Osc { frequency: 440Hz }
    module out : AudioOut

    osc.sine -> out.in_left
    osc.sine -> out.in_right
}
```

### Parameter values

| Syntax | Meaning |
| --- | --- |
| `440.0` | bare float |
| `2` | bare integer |
| `440Hz` / `2.5kHz` | frequency — converted to V/oct at parse time |
| `-6dB` | decibels — converted to linear amplitude |
| `C4` / `A#3` / `Bb2` | note name — converted to V/oct |
| `linear` | unquoted string (quotes optional: `"linear"` also works) |
| `true` / `false` | boolean |

Duration parameters (e.g. attack, release) take bare floats in seconds.

### Arity parameters

Some modules accept a variable number of ports, declared in parentheses:

```patches
module mix : StereoMixer(channels: 4) {
    level[0]: 0.8, pan[0]: -0.5,
    level[1]: 0.5, pan[1]:  0.7
}
```

### At-block syntax

Indexed parameters can be grouped per index using `@` blocks:

```patches
module dly : Delay(channels: 2) {
    @0: { delay_ms: 250, feedback: 0.4 },
    @1: { delay_ms: 375, feedback: 0.3 }
}
```

### Templates

Reusable sub-graphs can be defined as templates with typed parameters and
boundary ports (`$`). Templates are inlined by the expander before
interpretation.

```patches
template voice(freq: float = 440Hz) {
    in: gate, trigger, voct
    out: audio

    module osc : Osc { frequency: <freq> }
    module env : Adsr { attack: 0.01, decay: 0.1, sustain: 0.8, release: 0.3 }
    module vca : Vca

    $.gate    -> env.gate
    $.trigger -> env.trigger
    $.voct    -> osc.voct
    osc.sine  -> vca.in
    env.out   -> vca.cv
    $.audio   <- vca.out
}

patch {
    module v : voice(freq: 660Hz)
    module out : AudioOut

    v.audio -> out.in_left
    v.audio -> out.in_right
}
```

Template parameter types: `float`, `int`, `bool`, `str`. Parameters without
a default are required at every call site. Parameter references use angle
brackets: `<freq>`.

### FM synth example

```patches
patch {
    module kbd      : PolyMidiIn
    module osc_a    : PolyOsc { fm_type: "linear" }
    module osc_b    : PolyOsc { frequency: 200Hz }
    module vol_adsr : PolyAdsr
    module mod_adsr : PolyAdsr {
        attack: 0.005, decay: 0.5, sustain: 0.0, release: 0.1
    }
    module vca      : PolyVca
    module mod_vca  : PolyVca
    module to_mono  : PolyToMono
    module filter   : Lowpass { cutoff: 1200Hz, resonance: 0.0 }
    module out      : AudioOut

    kbd.trigger -> vol_adsr.trigger
    kbd.trigger -> mod_adsr.trigger
    kbd.gate    -> vol_adsr.gate
    kbd.gate    -> mod_adsr.gate
    kbd.voct    -> osc_a.voct
    kbd.voct    -> osc_b.voct

    # Carrier chain
    vol_adsr.out -> vca.cv
    osc_a.sine   -> vca.in
    vca.out      -> to_mono.in
    to_mono.out  -[0.05]-> filter.in
    filter.out   -> out.in_left
    filter.out   -> out.in_right

    # Modulator chain
    mod_adsr.out -> mod_vca.cv
    osc_b.sine   -> mod_vca.in
    mod_vca.out  -[0.3]-> osc_a.phase_mod
}
```

## Design constraints

- No allocations on the audio thread.
- No blocking on the audio thread (no mutexes, I/O, or syscalls in the
  processing path).
- `patches-core` has no knowledge of audio backends, file formats, or UI.
- Module descriptors are compile-time constants (`&'static str` port names);
  accessing them does not allocate.
