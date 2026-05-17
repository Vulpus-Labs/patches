---
id: "0901"
title: Manual — installation chapters
priority: medium
created: 2026-05-17
epic: E149
---

## Summary

Write the four Installation chapters: player, CLAP plugin, VS Code
extension, and the unsigned-binaries appendix. Each main chapter
covers both the prebuilt-artefact path and the build-from-source
path, with platform-specific caveats.

## Acceptance criteria

- [ ] `docs/src/install-player.md` — prebuilt download from GitHub
      release, `cargo install --path patches-player`, build from
      source, ALSA / JACK dev headers on Linux, verification step
      (`patches-player --version` and a play of `examples/
      square_440.patches`).
- [ ] `docs/src/install-clap.md` — prebuilt `.clap` bundle download
      with install paths per OS (`~/Library/Audio/Plug-Ins/CLAP/`,
      `%LOCALAPPDATA%\Programs\Common\CLAP\`, `~/.clap/`), macOS
      `xattr -dr com.apple.quarantine` instructions, Windows
      unblock-via-properties, build from source referencing
      `deploy.sh` as canonical macOS recipe, verification (DAW
      rescan, "Patches" + "Patches FX" appear under instruments +
      effects respectively).
- [ ] `docs/src/install-vscode.md` — VSIX install via
      `code --install-extension`, platform-specific VSIX naming
      (`patches-vscode-<platform>-<arch>-<ver>.vsix`), build from
      source via `vsce package` from `patches-vscode/`, LSP binary
      bundling per-platform, macOS quarantine strip on bundled LSP
      binary if it fails to start, verification (open a `.patches`
      file, syntax highlight + hover appear).
- [ ] `docs/src/install-unsigned.md` — single page explaining: no
      Apple Developer ID, no Windows code signing certificate
      currently; what each OS does (Gatekeeper, SmartScreen, none
      on Linux); the canonical override commands. Linked from each
      install chapter's caveat box.
- [ ] All four chapters render without dead links under
      `mdbook build`.

## Notes

- GH workflows that produce release artefacts: `build-macos.yml`,
  `build-windows.yml`, `release.yml`, `release-vsix.yml`.
- `deploy.sh` at repo root: canonical macOS local install for CLAP
  + VSIX. Read it before writing install-clap.
- VSIX naming reference: `dist/patches-vscode-darwin-arm64-0.0.1.vsix`.
- Windows MSI installer tracked separately as ticket
  `0900-windows-msi-installer.md` — mention as future work in
  install-player, do not block on it.
- Three-workflows framing: do not promote any surface over another.
  Player is first by install-simplicity order, not by being
  recommended.
