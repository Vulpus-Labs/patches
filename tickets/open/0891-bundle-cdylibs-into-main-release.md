---
id: "0891"
title: Bundle patches-bundles cdylibs into main repo's GitHub Release
priority: medium
created: 2026-05-14
---

## Summary

Main repo's release tarball currently ships `patch_player` + the CLAP
plugin only. Users who want the stdlib bundles (`patches-vintage`,
`patches-drums`, `patches-fft-bundle`) have to track the
[patches-bundles](https://github.com/Vulpus-Labs/patches-bundles)
repo separately, build the cdylibs themselves, and drop them into
`$EXE_DIR/plugins/`.

Wire patches-bundles' release artefacts into main repo's release
workflow so a single download gets users a working player + the
three stdlib bundles.

## Acceptance criteria

- [ ] `patches-bundles` ships a GitHub Release per `v0.7.x` tag
      with platform-specific archives containing
      `libpatches_vintage.{dylib,so,dll}`,
      `libpatches_drums.{dylib,so,dll}`, and
      `libpatches_fft_bundle.{dylib,so,dll}` (Linux + macOS arm64 +
      macOS x86_64 + Windows x86_64).
- [ ] Main repo's `.github/workflows/release.yml`:
  - Per-platform job downloads the matching patches-bundles
    archive via `gh release download` (or `curl` from the release
    URL); pins to a specific patches-bundles version compatible
    with the host's ABI (currently v12).
  - Stages the three cdylibs into `staging/plugins/` so the
    host's `PluginScanner::stdlib_scanner` picks them up from
    `$EXE_DIR/plugins/` per ticket 0876 + ADR 0073.
- [ ] Released tarball layout (per platform):
  ```text
  patches-VERSION-PLATFORM/
  ├── patch_player[.exe]
  ├── Patches.clap (CLAP bundle)
  ├── plugins/
  │   ├── libpatches_vintage.{dylib,so,dll}
  │   ├── libpatches_drums.{dylib,so,dll}
  │   └── libpatches_fft_bundle.{dylib,so,dll}
  ├── examples/
  ├── patches-manual.pdf
  └── (macOS only) macos-unquarantine.sh
  ```
- [ ] macOS ad-hoc codesign covers the three bundle dylibs as well
      as the host binaries.
- [ ] First release that ships with bundles works: drop the
      tarball on a clean machine, launch `patch_player` against a
      bundle-using `.patches` file (e.g. one of the examples now
      living in patches-bundles), audio renders.

## Notes

Dependency chain: patches-bundles must publish its GitHub Release
before main repo's release job runs. Two patterns work:

1. **Trigger-then-wait**: tag main repo, its release.yml fires,
   waits for matching patches-bundles release to exist (poll the
   GitHub API), then downloads.
2. **Manual ordering**: tag patches-bundles first, publish its
   release, then tag main repo. release.yml assumes the bundle
   release already exists.

Option 2 is simpler and matches how releases get cut in practice
(human-driven). Start there.

Version coupling: patches-bundles' version need not match main
repo's exactly — what matters is descriptor_hash / ABI version
compatibility (E145 / ABI v12). Pin patches-bundles to a known-good
version in release.yml; bump when patches-bundles releases a new
version that the host has validated.

Out of scope:

- Notarising bundle cdylibs (currently ad-hoc signed only;
  notarisation is a separate concern that applies to the whole
  release).
- Multi-version bundle selection (host can only load one
  patches-vintage at a time; users wanting an older bundle override
  via `PATCHES_PLUGIN_PATH`).
- Auto-update of bundles independent of host (defer until users
  ask).
