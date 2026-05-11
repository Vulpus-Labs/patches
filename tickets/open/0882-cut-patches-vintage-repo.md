---
id: "0882"
title: Cut patches-vintage repo
priority: medium
created: 2026-05-11
---

## Summary

Move patches-vintage out of the monorepo into its own repo. Already
a cdylib+rlib bundle ([patches-vintage/Cargo.toml](../../patches-vintage/Cargo.toml)),
already not in `default_registry()`, already loaded via PluginScanner.
Cut is mechanical.

Depends on ticket 0888 (patches-sdk + patches-core + patches-ffi-common
published to crates.io at 0.7.0). Bundle Cargo.toml uses
`patches-sdk = "0.7"` from crates.io.

## Acceptance criteria

- [ ] New repo `patches-vintage` initialised; single crate workspace.
- [ ] Deps: `patches-sdk = "0.7"` (crates.io) + `patches-dsp` (git
      tag from the main `patches` repo).
- [ ] `cargo build`, `cargo test`, `cargo clippy` green.
- [ ] cdylib build produces a loadable bundle; `descriptor_hash`
      matches ABI v12.
- [ ] CI scaffolded; release build of cdylib uploaded as a release
      artefact (Linux + macOS at minimum).
- [ ] `v0.7.0` tagged.
- [ ] Main repo: remove patches-vintage workspace member; host
      consumes the cdylib artefact via PluginScanner search path
      (already does — confirm).
- [ ] Main repo `just push` green.

## Notes

This is the simplest of the bundle cuts because vintage is already
in dual-mode shape and has its own integration history (ticket 0570,
ADR 0045 Spike 8 Phase C).

Confirm `patches-vintage::register()` survives in the rlib path —
[patches-vintage/src/lib.rs:47](../../patches-vintage/src/lib.rs#L47).
Comment says ticket 0570 was supposed to remove it once "Phase D
(bundle-load integration test) is green". Check status; remove if
done, retain if not.

Out of scope:

- Refactoring vintage modules.
- Adding new vintage modules.
