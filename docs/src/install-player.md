# Installing the player

`patches-player` is the standalone command-line player. Give it a
`.patches` file and it opens an audio device, runs the patch, watches
the file, and hot-reloads on save. Two install paths: a prebuilt
release archive, or a build from source.

## Prebuilt release archive

Each stable GitHub release attaches:

| Archive                                          | Use on                              |
| ------------------------------------------------ | ----------------------------------- |
| `patches-<version>-macos-aarch64.tar.gz`         | Apple Silicon Macs                  |
| `patches-<version>-macos-x86_64.tar.gz`          | Intel Macs                          |
| `patches-<version>-windows-x86_64.msi`           | 64-bit Windows (installer, default) |
| `patches-<version>-windows-x86_64.zip`           | 64-bit Windows (flat zip)           |

Prerelease tags ship only the zip on Windows; the MSI is built on stable
tags. Linux has no prebuilt artefact yet — see *Build from source* below.

Each archive contains:

- `patch_player` (the binary; `patch_player.exe` on Windows)
- `Patches.clap` — the CLAP plugin, covered in the next chapter
- a matching `.vsix` for the VS Code extension
- `examples/` — the patches under `examples/` in the repo
- `patches-manual.pdf` — this manual
- on macOS: `macos-unquarantine.sh`

### macOS

```bash
tar -xzf patches-<version>-macos-aarch64.tar.gz
cd patches-<version>-macos-aarch64
./macos-unquarantine.sh
./patch_player examples/square_440.patches
```

The binary is ad-hoc-signed but not notarised, so Gatekeeper marks it
quarantined on first download. `macos-unquarantine.sh` strips the
quarantine flag from `patch_player`, `Patches.clap`, any bundled
native module dylibs, and the bundled `.vsix`. As a convenience it
also installs `Patches.clap` into `~/Library/Audio/Plug-Ins/CLAP/`
and (if the `code` CLI is on PATH) installs the bundled VS Code
extension — covered in the next two chapters. Without the quarantine
strip the first run fails with a "cannot be opened because the
developer cannot be verified" dialog. See *Unsigned binaries* for the
underlying mechanism and manual overrides.

Drop `patch_player` somewhere on your `PATH` (e.g. `~/.local/bin/`,
`/usr/local/bin/`) if you want to run it without a path prefix.

### Windows

Two install paths. The MSI is the default — use it unless you specifically
want a portable, no-install copy.

**MSI (recommended).** Double-click `patches-<version>-windows-x86_64.msi`
and accept the elevation prompt. The installer is **not signed**, so
SmartScreen will warn on first run — click *More info → Run anyway*. It
places:

- `patch_player.exe`, the manual PDF, and the LICENSE into
  `C:\Program Files\Patches\`
- `Patches.clap` into `C:\Program Files\Common Files\CLAP\` (the standard
  CLAP system path)
- the bundled `.patches` examples into
  `C:\Users\Public\Documents\Patches\examples\`
- registers `.patches` as a file type whose default opener is
  `patch_player.exe`, so double-clicking a `.patches` file launches it
- adds an `App Paths` entry, so `patch_player` resolves from any cmd /
  PowerShell prompt without manual `PATH` editing
- adds a Start Menu *Patches* group with shortcuts to the examples folder
  and the manual

The VS Code extension is **not** included in the MSI — install it via
*Extensions → Install from VSIX…* using the `.vsix` from the zip archive,
or from the Marketplace if/when it lands there.

**Zip (portable).** Extract `patches-<version>-windows-x86_64.zip` with
Explorer or `tar -xf` from PowerShell. Right-click `patch_player.exe` →
*Properties* → tick *Unblock* before first run, or SmartScreen will
refuse to launch it. Then:

```cmd
patch_player.exe examples\square_440.patches
```

The zip is the lightweight option — no installer, no registry changes,
no Start Menu entries — and it stays available alongside the MSI on
every stable release.

### Linux

No prebuilt artefact yet. Build from source.

## Build from source

The player builds on macOS, Linux, and Windows with a stable Rust
toolchain. From a checkout of the repo:

```bash
cargo install --path patches-player
```

This installs `patch_player` into `~/.cargo/bin/`. If that directory is
on your `PATH`, the binary is immediately runnable.

To build without installing — useful when iterating on the source —
use `cargo build --release -p patches-player`; the binary lands at
`target/release/patch_player`.

### Linux dependencies

CPAL needs ALSA development headers. On Debian/Ubuntu:

```bash
sudo apt install libasound2-dev
```

On Fedora:

```bash
sudo dnf install alsa-lib-devel
```

JACK is supported through ALSA's JACK bridge or PipeWire's JACK
shim — no separate dependency on the JACK library is required.

### macOS / Windows

No system packages required beyond a working Rust toolchain
(`rustup default stable`). On Windows, `cargo install` uses the MSVC
toolchain by default; the GNU toolchain is untested.

## Verification

```bash
patch_player --version    # not yet implemented; use the next check
patch_player examples/square_440.patches
```

A short pulse-width-modulated square at ~100 Hz plays through the
default output device. Ctrl-C to stop. If you see the TUI, hear the
tone, and see "audio: started" in the log pane, the player is working.

If the player exits with "no default output device", pass
`--output-device <name>` (run with `--list-devices` to see what is
available).
