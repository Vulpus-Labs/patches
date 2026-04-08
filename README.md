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
    module env : PolyAdsr   { attack: 0.005, decay: 0.1, sustain: 0.7, release: 0.3 }
    module vca : PolyVca
    module mix : PolyToMono
    module out : AudioOut

    kbd.trigger -> env.trigger
    kbd.gate    -> env.gate
    kbd.v_oct   -> osc.voct

    osc.sine    -> vca.in
    env.out     -> vca.cv
    vca.out     -> mix.in
    mix.out -[0.2]-> out.left
    mix.out -[0.2]-> out.right
}
```

Save this as `hello.patches`, connect a MIDI device, then run:

```bash
cargo run -p patches-player -- hello.patches
```

`PolyMidiIn` uses the first available MIDI input port. The device must be
connected before the process starts. While it is running, edit and save
`hello.patches` to hot-reload the patch without interrupting audio.

## Goals

- **Patch DSL** — a DSL for describing signal graphs of audio modules
  (connections, scaling, routing, polyphony, and reusable templates).
- **Audio engine** — a real-time processing pipeline that accepts new patch plans
  without allocating or blocking on the audio thread.
- **Live-reload** — stateful module instances survive re-planning; only structurally
  changed parts of the graph are reset.

## Current state

The full `.patches` DSL is implemented. `patches-player` watches its input file
and re-plans the patch (keeping existing modules running) whenever a new version
is saved.

The core engine and a practical set of modules are in place:

- **`patches-dsl`** — PEG parser and template expander for the `.patches` DSL.
  Produces a `FlatPatch` (modules + edges) with no knowledge of concrete module
  types. Supports: module declarations, inline parameters, typed template
  parameters with defaults, indexed ports (`in[1]`), scaled connections
  (`-[0.5]->`), and `$` boundary ports for templates.
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

### Modules

| Module | Description |
| --- | --- |
| `Osc` | Mono oscillator (sine, sawtooth, square, triangle) |
| `PolyOsc` | Polyphonic oscillator (sine, sawtooth, square, phase mod) |
| `Lfo` | Low-frequency oscillator (sine output) |
| `Adsr` | Mono ADSR envelope |
| `PolyAdsr` | Per-voice ADSR envelope |
| `Vca` | Mono voltage-controlled amplifier |
| `PolyVca` | Per-voice VCA |
| `Lowpass`, `Highpass`, `Bandpass` | Mono resonant filters |
| `PolyLowpass`, `PolyHighpass`, `PolyBandpass` | Per-voice resonant filters |
| `Sum` | Multi-input mono mixer |
| `PolyMix` | Multi-input poly mixer |
| `PolyToMono` | Collapse poly voices to a mono signal |
| `MonoToPoly` | Broadcast a mono signal to all poly voices |
| `MidiIn` | Mono MIDI note input (gate, trigger, V/oct) |
| `PolyMidiIn` | Polyphonic MIDI input with LIFO voice stealing |
| `Glide` | Portamento / pitch smoothing |
| `Tuner` | Pitch offset (octave + cent) |
| `Clock` | Clock pulse generator |
| `Seq` | Step sequencer |
| `AudioOut` | Stereo audio output |

## Workspace layout

```text
patches-core/              Core types, traits, and execution plan runtime.
                           No audio-backend dependencies; fully testable without hardware.

patches-dsl/               PEG parser and template expander for the .patches DSL.
                           No knowledge of concrete module types.

patches-interpreter/       Validates FlatPatch against the module registry;
                           constructs ModuleGraph.

patches-modules/           Audio module implementations (oscillators, filters, effects, …).

patches-engine/            Patch builder, Planner, PatchEngine, CPAL sound engine,
                           and runnable examples.

patches-player/            `patch_player` binary: load a patch, play it, hot-reload
                           on file change.

patches-integration-tests/ Cross-crate integration tests (not published).

tickets/                   Work tracking (open / in-progress / closed).
epics/                     Epics grouping related tickets.
adr/                       Architecture decision records.
```

## Building and running

```bash
cargo build
cargo test
cargo clippy

# Run the player with a patch file (hot-reloads on save):
cargo run -p patches-player -- examples/fm_synth.patches
cargo run -p patches-player -- examples/poly_synth.patches
cargo run -p patches-player -- examples/poly_synth_layered.patches
```

## Patch format

Patches are written in the `.patches` DSL. Modules are declared with optional
inline parameters; connections use `->` or `-[scale]->` for scaled cables.
Indexed ports use `name[n]` notation.

```patches
patch {
    module osc : SineOscillator { frequency: 440.0Hz }
    module out : AudioOut

    osc.out -> out.left
    osc.out -> out.right
}
```

### Templates

Reusable sub-graphs can be defined as templates with typed parameters and
boundary ports (`$`). Templates are inlined by the expander before interpretation.

```patches
template voice(freq: float = 440.0Hz) {
    out: audio

    module osc : SineOscillator { frequency: <freq> }
    module env : Adsr           { attack: 0.01, decay: 0.1, sustain: 0.8, release: 0.3 }
    module vca : Vca

    osc.out     -> vca.in
    env.out     -> vca.cv
    $.audio <- vca.out
}

patch {
    module v : voice(freq: 660.0Hz)
    module out : AudioOut

    v.audio -> out.left
    v.audio -> out.right
}
```

### FM synth example

```patches
patch {
    module kbd      : PolyMidiIn
    module osc_a    : PolyOsc { fm_type: "linear" }
    module osc_b    : PolyOsc { frequency: 200.0Hz }
    module vol_adsr : PolyAdsr
    module mod_adsr : PolyAdsr { attack: 0.005, decay: 0.5, sustain: 0.0, release: 0.1 }
    module vca      : PolyVca
    module mod_vca  : PolyVca
    module to_mono  : PolyToMono
    module filter   : Lowpass { cutoff: 1200.0Hz, resonance: 0.0 }
    module out      : AudioOut

    kbd.trigger -> vol_adsr.trigger
    kbd.trigger -> mod_adsr.trigger
    kbd.gate    -> vol_adsr.gate
    kbd.gate    -> mod_adsr.gate
    kbd.v_oct   -> osc_a.voct
    kbd.v_oct   -> osc_b.voct

    # Carrier chain
    vol_adsr.out -> vca.cv
    osc_a.sine   -> vca.in
    vca.out      -> to_mono.in
    to_mono.out  -[0.05]-> filter.in
    filter.out   -> out.left
    filter.out   -> out.right

    # Modulator chain
    mod_adsr.out -> mod_vca.cv
    osc_b.sine   -> mod_vca.in
    mod_vca.out  -[0.3]-> osc_a.phase_mod
}
```

## Design constraints

- No allocations on the audio thread.
- No blocking on the audio thread (no mutexes, I/O, or syscalls in the processing path).
- `patches-core` has no knowledge of audio backends, file formats, or UI.
- Module descriptors are compile-time constants (`&'static str` port names); accessing them does not allocate.
