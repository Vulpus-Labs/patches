# patches

A modular software synthesizer you program with text. Describe a graph of
oscillators, filters, envelopes, sequencers, and effects in a small patch
language; run it as a standalone instrument, as a CLAP plugin in your DAW,
or as a self-contained sequenced piece. Edit the file while it plays and
the sound updates without an audio dropout — the whole system is built for
live patching.

📖 **Manual:** <https://vulpus-labs.github.io/patches/>

```patches
patch {
    module kbd : PolyMidiToCv
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
    mix.out -[0.2]-> out.in
}
```

That's a complete polyphonic MIDI synth: plug in a keyboard and play.

## What you can do with it

- **Patch like a modular.** Wire modules together with cables, exactly as
  you would on a hardware rack — except your rack is a text file, every
  patch is reproducible, and you can put it under version control.
- **Play it live.** The standalone player watches your file; saving
  applies the new patch while audio keeps running. Tweak a filter
  cutoff, rewire an LFO, add a delay — mid-performance.
- **Sequence whole pieces.** A built-in tracker-style sequencer supports
  patterns, songs, note slides, and rolls, so a single `.patches` file
  can be an entire composition that renders to WAV.
- **Go polyphonic for free.** Poly modules carry up to 16 voices per
  cable; a mono patch becomes polyphonic by swapping module types, not by
  duplicating wiring.
- **Use it in your DAW.** The CLAP plugin runs the same patches inside a
  host, with patch-defined knobs, sliders, and toggles exposed as
  automatable plugin parameters.
- **See what's happening.** Tap any cable to get meters, oscilloscope,
  and spectrum views — in the player's built-in terminal UI or the
  plugin GUI.

## The module library

A broad set of building blocks ships in the box:

- **Oscillators** — band-limited mono and poly oscillators, FM operators,
  supersaw, LFOs, noise sources
- **Filters** — classic ladder-style and state-variable filters, EQ
  biquads, in mono, stereo, and poly flavours
- **Envelopes & modulation** — ADSR, multi-stage envelopes, sample &
  hold, glide, ring modulation, pitch quantizers
- **Effects** — delays, reverb, drive/waveshaping, bitcrusher
- **Dynamics** — compressors, limiters, noise gates, and a transient
  shaper, in mono and stereo, with sidechain support
- **Sequencing & MIDI** — pattern/tracker playback, clocks, MIDI-to-CV
  conversion (mono and polyphonic), drum trigger mapping,
  audio-to-gate/trigger detectors
- **Utilities** — mixers, VCAs, stereo tools, signal math

More modules live in the
[patches-bundles](https://github.com/Vulpus-Labs/patches-bundles) repo,
loaded at runtime as plugin bundles: vintage-style analogue emulations
(chorus, flangers, bucket-brigade delays, DCOs, classic filter models), a
full drum kit (kick, snare, hats, toms, cymbals, and more), convolution
reverb, and pitch shifting.

You can also extend the system with native plugin modules via a small
SDK — the bundles above are built with it — and patches themselves
support reusable templates so you can build your own higher-level voices
and instruments out of the primitives.

## Editor support

A VS Code extension provides syntax highlighting, live diagnostics as you
type, hover documentation for modules and ports, go-to-definition, and a
rendered diagram of your patch graph.

## Install

Three surfaces — install whichever you need. All artefacts are unsigned;
see the manual for OS-specific workarounds (Gatekeeper quarantine on
macOS, SmartScreen on Windows).

```bash
# Standalone player (any OS)
cargo install --path patches-player

# CLAP plugin — macOS local install
./deploy.sh

# VS Code extension
code --install-extension patches-vscode-<platform>-<arch>-<ver>.vsix
```

Prebuilt binaries for all three are attached to GitHub releases.

## Run

```bash
patch_player hello.patches
```

(Note: the crate is `patches-player`; the installed binary is `patch_player`.)

The player opens a terminal UI with meters, scopes, and an event log, and
watches the file: save changes to hot-reload without interrupting audio.
Pass `--record out.wav` to capture the performance.

## Licence

See [LICENSE](LICENSE).
