# Installing the CLAP plugin

The CLAP build is a single dynamic library that ships two descriptors:
**Patches** (an instrument) and **Patches FX** (an audio effect). Most
hosts surface them in two slots — instruments and effects — from the
same `.clap` bundle. No separate effect build, no extra install step.

## Prebuilt release archive

The release archives described in the previous chapter contain
`Patches.clap` alongside `patch_player`. To install just the CLAP
plugin from those archives, copy the bundle into your CLAP search
directory:

| OS      | Install path                                        |
| ------- | --------------------------------------------------- |
| macOS   | `~/Library/Audio/Plug-Ins/CLAP/Patches.clap`        |
| Windows | `%LOCALAPPDATA%\Programs\Common\CLAP\Patches.clap`  |
| Linux   | `~/.clap/Patches.clap`                              |

### macOS

```bash
tar -xzf patches-<version>-macos-aarch64.tar.gz
cd patches-<version>-macos-aarch64
./macos-unquarantine.sh
```

`macos-unquarantine.sh` strips the Gatekeeper quarantine flag from
`Patches.clap` and copies it into `~/Library/Audio/Plug-Ins/CLAP/`.
For a manual install, skip the script and run:

```bash
mkdir -p ~/Library/Audio/Plug-Ins/CLAP
cp -R Patches.clap ~/Library/Audio/Plug-Ins/CLAP/
xattr -dr com.apple.quarantine ~/Library/Audio/Plug-Ins/CLAP/Patches.clap
```

If a host rejects the plugin with "cannot be opened" after install,
repeat the `xattr -dr` step and rescan.

### Windows

Right-click `Patches.clap` → *Properties* → *Unblock*, then copy:

```cmd
mkdir "%LOCALAPPDATA%\Programs\Common\CLAP"
copy Patches.clap "%LOCALAPPDATA%\Programs\Common\CLAP\"
```

The system-wide path `C:\Program Files\Common Files\CLAP\` works too
but requires elevation.

### Linux

No prebuilt artefact yet — build from source.

## Build from source

The fastest macOS recipe is the `deploy.sh` script at the repo root.
It release-builds `patches-clap`, strips quarantine from any previous
install, and copies the dylib into
`~/Library/Audio/Plug-Ins/CLAP/Patches.clap`. It also packages and
installs the VS Code extension; read the script first if you only
want the plugin.

```bash
./deploy.sh
```

For the underlying steps on any OS:

```bash
cargo build --release -p patches-clap
```

This produces a platform-specific dylib in `target/release/`:

| OS      | Built artefact            | Copy to                                              |
| ------- | ------------------------- | ---------------------------------------------------- |
| macOS   | `libpatches_clap.dylib`   | `~/Library/Audio/Plug-Ins/CLAP/Patches.clap`         |
| Linux   | `libpatches_clap.so`      | `~/.clap/Patches.clap`                               |
| Windows | `patches_clap.dll`        | `%LOCALAPPDATA%\Programs\Common\CLAP\Patches.clap`   |

The destination filename ends in `.clap` regardless of source
extension — hosts identify the bundle by name and load whatever shared
object is inside.

### Linux dependencies

Same as the player: ALSA dev headers
(`sudo apt install libasound2-dev` or equivalent).

## Verification

1. Start your DAW (REAPER, Bitwig, Ableton 11.3+, FL Studio 21.2+,
   Studio One 6.5+, …).
2. Trigger a plugin rescan if the host doesn't scan on startup.
3. **Patches** should appear under the *Instruments* category.
4. **Patches FX** should appear under the *Effects* (or *Audio
   Effects*) category.
5. Load **Patches** on a MIDI track, open its UI, and load a patch
   from `examples/` via the file picker.

If only one of the two descriptors shows up, the host is likely
filtering by feature tag; check both the instrument and effect
sections of the plugin browser. The dylib is the same — no second
install needed.

If neither shows up:

- macOS — quarantine. `xattr -l ~/Library/Audio/Plug-Ins/CLAP/Patches.clap`
  will list it if present; re-run `macos-unquarantine.sh` or strip
  manually.
- Windows — *Unblock* the file via Properties; SmartScreen blocks
  load until you do.
- All OSes — confirm the host is actually scanning the directory you
  copied to. Some DAWs maintain a per-host CLAP path list that
  doesn't include the user-local default.
