# Patches v0.7.0

Patches is a modular audio synthesiser system for live-coding performance.
You describe a signal graph of modules in a plain-text `.patches` file; the
engine builds and runs the graph in real time. Edit and save the file while
audio is playing — the engine re-plans the graph and swaps it in without
interrupting the audio stream or resetting module state.

## Getting started

Download the archive for your platform, unpack it, and run:

```sh
patch_player examples/radigue_drone.patches
```

Edit the file while it plays. Changes take effect on save.

## macOS — first-run step

macOS will block the binary because it is not signed with an Apple
certificate. Run this once after unpacking:

```sh
xattr -dr com.apple.quarantine patch_player
```

After that, `patch_player` runs normally.

## What's new since v0.2

### Full `.patches` DSL

The DSL has matured into a complete language for describing modular
signal graphs:

- Module declarations with named parameters and unit literals
  (`440Hz`, `-6dB`, `C4`, `A#3`)
- Scaled connections (`-[0.5]->`)
- Typed template parameters (`float`, `int`, `bool`, `str`) with defaults
- Variable-arity templates (`[*n]`) for building N-voice structures
- Indexed ports (`in[1]`) and at-blocks (`@0: { ... }`) for grouping
  per-index parameters
- Template boundary ports (`$`) and param references (`<freq>`)
- `include` directives for reusing templates across files

### Module library

The module set has grown from 14 to 60+. Highlights:

- **Oscillators:** `Osc`, `PolyOsc`, `Lfo`, `PolyLfo` with FM, phase-mod,
  drift, and sync inputs
- **Filters:** `Lowpass`/`Highpass`/`Bandpass` biquads, `Svf` state-variable
  filter (mono and poly variants)
- **Envelopes:** `Adsr`, `PolyAdsr`
- **Amplifiers/mixers:** `Vca`, `PolyVca`, `Sum`, `PolySum`, `Mixer`,
  `StereoMixer`, `PolyMixer`, `StereoPolyMixer`, `MonoToPoly`, `PolyToMono`
- **MIDI:** `MidiToCv`, `PolyMidiToCv` (with LIFO voice stealing), `MidiCC`,
  `MidiArp`, `MidiDelay`, `MidiSplit`, `MidiTranspose`, `MidiDrumset`
- **Sequencing:** `Clock`, `MasterSequencer`, `PatternPlayer`, `TempoSync`,
  `MsTicker`, `TriggerToSync`, `SyncToTrigger`
- **Drum synthesis:** `Kick`, `Snare`, `Clap`, `ClosedHiHat`, `OpenHiHat`,
  `Tom`, `Cymbal`, `Claves`
- **Delays & reverb:** `Delay`, `StereoDelay` (multi-tap with per-tap
  feedback/drive), `FdnReverb` (plate/room/chamber/hall/cathedral),
  `ConvReverb`, `StereoConvReverb` (built-in IRs or file)
- **Dynamics/effects:** `Limiter`, `StereoLimiter`, `Bitcrusher`, `Drive`,
  `TransientShaper`, `RingMod`, `PitchShift` (spectral WOLA phase vocoder
  with optional formant preservation)
- **Utilities:** `Glide`, `Tuner`, `PolyTuner`, `Quant`, `PolyQuant`, `Sah`,
  `PolySah`

### Hot-reload engine

- Off-thread plan compilation; lock-free plan handoff via rtrb ring buffer
- Module state (oscillator phase, filter history, envelope stage) carried
  forward across reloads; only structurally changed sub-graphs reset
- Cleanup thread for deferred drops — audio thread never deallocates
- `ParamFrame` + `ArcTable` data plane for parameter delivery without
  locks (ADR 0045)
- `CoefRamp` / `PolyCoefRamp` primitives eliminate zipper noise on
  filter-coefficient changes; biquad/SVF/ladder kernels migrated (ADR 0050)

### CLAP plugin host

`patches-clap` ships Patches as a CLAP plugin for DAWs (Reaper, Studio
One, Ableton, Bitwig). Includes patch persistence, hot-reload, MIDI note
and CC input, flexible audio routing, and an in-GUI path editor with
module rescan.

### Native FFI plugin system

Modules can be loaded as dynamic libraries (ADR 0045, spikes 0–9):

- Stable `repr(C)` ABI, `ABI_VERSION = 5`
- Compile-time descriptor hashing for host/plugin alignment
- `export_plugin!` macro for boilerplate-free plugin definition
- `ParamFrame` / `ArcTable` fuzz targets + 10k-cycle alloc-trap soak
  (E111)
- Module panics caught at the tick boundary; engine halts cleanly with
  attribution breadcrumb instead of taking down the host (ADR 0051, E113)
- `WANTS_PERIODIC` const + default `Module::periodic_update` replaces the
  raw-pointer `as_periodic()` footgun (ADR 0052, E114)

### VS Code extension & LSP

`patches-lsp` provides diagnostics, hover, and go-to-definition for
`.patches` files, plus custom `patches/renderSvg` and
`patches/rescanModules` methods. Bundled into platform-specific
`.vsix` packages — no separate install needed.

### Player & I/O

`patches-player` gains:

- `--oversampling 1|2|4|8` for anti-aliased high-frequency content
- `--record out.wav` for offline bounce
- `--output-device` / `--input-device` / `--list-devices`
- `--module-path` for loading FFI plugin bundles
- `--no-stdin` for headless operation

## VS Code extension

Platform-specific `.vsix` packages are attached to this release. Each
bundles the `patches-lsp` binary for its platform.

### Install

```sh
code --install-extension patches-vscode-<platform>-<version>.vsix
```

### macOS — removing quarantine from the LSP binary

```bash
xattr -d com.apple.quarantine ~/.vscode/extensions/vulpus-labs.patches-vscode-*/server/patches-lsp
```

Or: **System Settings > Privacy & Security** → find `patches-lsp` in
the Security section → **Allow Anyway**.

### Windows — unblocking the LSP binary

Windows SmartScreen may block the binary on first use.

1. Navigate to `%USERPROFILE%\.vscode\extensions\vulpus-labs.patches-vscode-*\server\`
2. Right-click `patches-lsp.exe` → **Properties** → check **Unblock** → **OK**

Or click **More info** → **Run anyway** when SmartScreen prompts.

### Workaround: custom binary path

If the bundled binary doesn't work:

```sh
cargo build --release -p patches-lsp
```

Then set `patches.lsp.path` in VS Code settings to the built binary path.

## Known limitations

- **macOS binaries are ad-hoc signed only** — see the first-run step above.
- **ASIO not supported on Windows** — uses WASAPI. Comment on the GitHub
  issue if ASIO support matters to you.
- **External FFI plugin diagnostics are sparse** — descriptor-hash
  mismatches are silently rejected, and malformed parameter frames are
  silently truncated in release builds. Both become strict errors in v0.8.
- The legacy YAML patch format is still supported but no longer the
  primary format. New patches should use `.patches`.
