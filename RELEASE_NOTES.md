# Patches v0.2

Patches is a modular audio synthesiser system for live-coding performance. You
describe a signal graph of modules in a plain-text `.patches` file; the engine
builds and runs the graph in real time. Edit and save the file while audio is
playing — the engine re-plans the graph and swaps it in without interrupting the
audio stream or resetting module state.

## Getting started

Download the archive for your platform, unpack it, and run:

```
patch_player examples/radigue_drone.patches
```

Edit the file while it plays. Changes take effect on save.

## macOS — first-run step

macOS will block the binary because it is not signed with an Apple certificate.
Run this once after unpacking:

```sh
xattr -dr com.apple.quarantine patch_player
```

After that, `patch_player` runs normally.

## What's in this release

### `.patches` DSL

A PEG-based domain-specific language for describing patches. Features include:

- Module instantiation with named parameters and unit literals (`440Hz`, `1s`, `0.5`)
- Named cable connections with optional scale factors
- Polyphonic cables (declared with `poly:` on a connection)
- Reusable **templates** with typed parameters, expanded at compile time
- **Param-ref syntax** for passing a parameter value through to a nested module
- **Variable-arity templates** for building N-voice structures without repetition

Example:

```patches
patch {
    module osc : Osc { frequency: 440Hz }
    module env : Adsr { attack: 10ms, decay: 100ms, sustain: 0.6, release: 200ms }
    module vca : Vca
    module out : AudioOut

    osc.sine -> vca.input
    env.output -> vca.cv
    vca.output -> out.left
    vca.output -> out.right
}
```

### Module library

| Module | Description |
|---|---|
| `Osc` | Anti-aliased oscillator (sine, saw, square, triangle) with v/oct pitch, FM, PWM, and phase-mod inputs |
| `Lfo` | Low-frequency oscillator with sync and rate-CV |
| `Adsr` | ADSR envelope generator |
| `Vca` | Voltage-controlled amplifier |
| `MonoMixer` / `StereoMixer` | Mono and stereo summing mixers |
| `PolyMixer` / `StereoPolyMixer` | Polyphonic summing mixers |
| `ResonantLowpass` / `Highpass` / `Bandpass` | Biquad filters (mono and poly variants) |
| `MonophonicMidiKeyboard` | Reads MIDI note-on/off; outputs pitch (v/oct) and gate |
| `ClockSequencer` / `StepSequencer` | Clock-driven step sequencers |
| `MonoToPoly` | Broadcasts a mono signal to all voices of a poly cable |
| `AudioOut` | Stereo audio output sink |

### Audio engine

- **Off-thread deallocation** — evicted modules and execution plans are dropped
  on a dedicated cleanup thread, never on the audio thread
- **Polyphonic cables** — up to 16 voices per cable, zero-allocation in the
  processing path
- **CablePool** — all cable reads and writes go through a ping-pong pool; no
  per-sample allocation
- **Lock-free plan handoff** — new execution plans are delivered to the audio
  thread via a single-producer single-consumer ring buffer
- **MIDI integration** — sub-block event scheduling with sample-accurate timing

### Example patches

| File | Description |
|---|---|
| `radigue_drone.patches` | Slowly evolving drone inspired by Éliane Radigue |
| `fm_synth.patches` | Two-operator FM synthesiser |
| `poly_synth.patches` | Four-voice polyphonic synthesiser |
| `poly_synth_layered.patches` | Layered polyphonic patch with filter envelopes |
| `demo_synth.yaml` | MIDI-driven demo synth (legacy YAML format) |

## VS Code extension

Platform-specific `.vsix` packages are attached to this release. Each package
bundles the `patches-lsp` binary for its platform — no separate installation
needed.

### Install the extension

Download the `.vsix` for your platform and run:

```sh
code --install-extension patches-vscode-<platform>-<version>.vsix
```

### macOS — removing quarantine from the LSP binary

The bundled `patches-lsp` binary is not signed with an Apple Developer
certificate. macOS will quarantine it on first launch and the extension will
warn that the LSP failed to start. Two ways to fix this:

**Option A — Terminal command (recommended):**

```bash
xattr -d com.apple.quarantine ~/.vscode/extensions/vulpus-labs.patches-vscode-*/server/patches-lsp
```

**Option B — System Settings:**

1. Open **System Settings > Privacy & Security**.
2. Scroll to the **Security** section — you should see a message about
   `patches-lsp` being blocked.
3. Click **Allow Anyway**, then try activating the extension again.

### Windows — unblocking the LSP binary

Windows SmartScreen may block the binary on first use.

**Option A — Properties dialog:**

1. Navigate to the extension folder (typically
   `%USERPROFILE%\.vscode\extensions\vulpus-labs.patches-vscode-*\server\`).
2. Right-click `patches-lsp.exe`, open **Properties**.
3. Check **Unblock** at the bottom of the General tab, click OK.

**Option B — SmartScreen prompt:**

When the binary is blocked at launch, click **More info** then **Run anyway**.

### Workaround: custom binary path

If the bundled binary doesn't work, you can build `patches-lsp` from source
and point the extension at it:

1. `cargo build --release -p patches-lsp`
2. In VS Code settings, set `patches.lsp.path` to the path of the built binary.

## Known limitations

- **macOS binaries are ad-hoc signed only** — see the first-run step above.
- **ASIO not supported on Windows** — uses WASAPI. Add a comment to the GitHub
  issue if ASIO support matters to you.
- The legacy YAML patch format is still supported but no longer the primary
  format. New patches should use `.patches`.
