# patches

A modular audio instrument with a text-based patch language. Describe a
graph of oscillators, filters, envelopes, sequencers, and effects; run it
as a CLAP plugin in a DAW, as a standalone instrument, or as a
self-contained sequenced piece.

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

The player watches the file. Save changes to hot-reload without interrupting audio.

## Licence

See [LICENSE](LICENSE).
