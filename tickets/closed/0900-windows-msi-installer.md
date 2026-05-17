---
id: "0900"
title: Windows MSI installer for release builds
priority: medium
created: 2026-05-16
---

## Summary

The Windows release build today ships a flat `.zip` of binaries
([release.yml#L224-L230](.github/workflows/release.yml#L224-L230)). Users
have to copy `patch_player.exe`, `Patches.clap`, and the examples to the
right places by hand, and `.patches` files have no default association.
Ship a real Windows installer instead: an MSI that places each artifact
in its conventional location, registers `.patches` as a file type whose
default opener is `patch_player.exe`, and surfaces the examples in a
discoverable folder.

## Acceptance criteria

- [ ] `cargo-wix` (WiX Toolset v3) wired into `patches-player` (or a
      dedicated `installers/windows/` directory) as the MSI generator.
      `wix/main.wxs` checked in. Build is reproducible from
      `cargo wix --no-build --install-version <ver>` after the release
      binaries are already produced.
- [ ] Installer is per-machine, requires elevation, ships
      x86_64 only (matches `release.yml` matrix).
- [ ] Component layout:
      - `%ProgramFiles%\Patches\patch_player.exe`
      - `%ProgramFiles%\Patches\patches-manual.pdf`
      - `%ProgramFiles%\Patches\LICENSE.txt` (root workspace LICENSE)
      - `%CommonProgramFiles%\CLAP\Patches.clap` — standard CLAP system
        path (the CLAP spec lists `Program Files\Common Files\CLAP`
        as one of the default scan locations on Windows)
      - `%PUBLIC%\Documents\Patches\examples\` — bundled `.patches`
        examples. Public Documents is used in place of per-user
        `My Documents` because a per-machine MSI cannot reliably write
        into a specific user's profile at install time; Public
        Documents is the conventional shared location and shows up
        under "Documents" in File Explorer for every user.
- [ ] `.patches` file association:
      - `HKLM\Software\Classes\.patches` (default) = `Patches.patchfile`
      - `HKLM\Software\Classes\Patches.patchfile` (default) = `Patches Patch`
      - `HKLM\Software\Classes\Patches.patchfile\DefaultIcon` =
        `<install>\patch_player.exe,0` (icon resource added to the
        binary; see Notes)
      - `HKLM\Software\Classes\Patches.patchfile\shell\open\command` =
        `"<install>\patch_player.exe" "%1"`
      - `HKLM\Software\Classes\Applications\patch_player.exe\SupportedTypes`
        lists `.patches` so the "Open with" menu surfaces it.
- [ ] `App Paths` registry entry
      (`HKLM\Software\Microsoft\Windows\CurrentVersion\App Paths\patch_player.exe`)
      so `patch_player` resolves from any cmd / PowerShell prompt
      without manual PATH editing.
- [ ] Start Menu shortcut group `Patches\` with:
      - shortcut to "Patches Examples" pointing at
        `%PUBLIC%\Documents\Patches\examples\`
      - shortcut to the manual PDF
      No shortcut for `patch_player.exe` itself — it is a
      command-line tool with no GUI entry point, and the file
      association handles the common path.
- [ ] Clean uninstall: every component, every registry key, and the
      Start Menu group removed. The examples folder under Public
      Documents is removed only if empty (so user-added patches
      survive). Verified by `msiexec /x` on a clean VM.
- [ ] Upgrade behaviour: installing a newer MSI over an older one
      replaces all files and updates the registry without prompting
      the user to uninstall first. Use a fixed `UpgradeCode` GUID
      (checked in to `main.wxs`) and bump `ProductVersion` from the
      release tag.
- [ ] `release.yml` Windows job builds the MSI after staging artifacts
      and uploads it alongside the existing zip. `package-and-release`
      includes `patches-<ver>-windows-x86_64.msi` in the GitHub
      Release assets.
- [ ] Manual smoke on a Windows 10 / 11 VM: install, double-click an
      example `.patches` file, confirm `patch_player.exe` launches
      and plays. Reload a CLAP host (Bitwig / Reaper) and confirm
      `Patches` and `Patches FX` show up under their respective
      categories.

## Notes

### Why cargo-wix

WiX itself is the de facto Windows MSI authoring toolchain. `cargo-wix`
is a thin wrapper that drives `candle` / `light` from a `Cargo.toml`
hint and a hand-written `wix/main.wxs`, so the heavy lifting still
lives in WiX XML. Alternatives considered:

- **NSIS** — produces a `.exe` installer, lighter weight, but the
  output is harder to integrate with Group Policy / silent enterprise
  deploys and the upgrade story is rougher.
- **Inno Setup** — similar trade-off to NSIS; not natively MSI.
- **`cargo-bundle`** — Windows support is incomplete; doesn't cover
  file associations.

cargo-wix is also what the rest of the Rust ecosystem leans on
(`ripgrep`, `rustup`, etc.), so prior art is plentiful.

### Icon resource

`patches-player/build.rs` already embeds `assets/patches.ico` into
`patch_player.exe` as resource index 0 via the `winresource` crate
(target-gated build-dep). The DefaultIcon registry value
`<install>\patch_player.exe,0` will resolve to this icon. No further
work on the binary side; the installer only needs to set the registry
key.

### My Documents vs Public Documents

A per-machine MSI runs as SYSTEM and cannot target a specific
user's `%USERPROFILE%\Documents` deterministically (which user?
all users? only the installer?). The conventional Windows patterns
for "examples that appear in Documents" are:

1. Install to `%PUBLIC%\Documents\Patches\examples\` (this ticket).
   Visible to every user.
2. Active Setup / "self-healing" first-run that copies from a
   read-only template into each user's profile the first time
   they launch the app.
3. Have `patch_player.exe` itself copy examples on first run.

Option 1 is the lowest-effort and matches what most audio tools
ship. If a future ticket really wants per-user copies, (3) is the
cleanest follow-up — installer-side first-run hooks
(option 2) are fragile.

### Bundle cdylibs

[ticket 0891](0891-bundle-cdylibs-into-main-release.md) will add
`patches-vintage.pxm`, `patches-drums.pxm`, etc. to the release. When
that lands, this installer should also place them under
`%ProgramData%\Patches\bundles\` (the per-machine equivalent of the
default bundle dir from [ADR 0075](../../adr/0075-global-host-config-for-bundle-dirs.md)).
The `PluginScanner` default tier uses `ProjectDirs` which resolves to
`%APPDATA%\Patches\bundles` (per-user) on Windows; the installer needs
to write to a per-machine path. Either:

- Ship bundles to `%ProgramData%\Patches\bundles` and add that to
  the scanner's default tier, or
- Drop them next to the binaries under
  `%ProgramFiles%\Patches\bundles\` and have the host pass the path
  in via `paths` at startup.

Defer the choice to 0891; this ticket only needs to leave room for
either.
