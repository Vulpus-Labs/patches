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

- [x] New repo `patches-vintage` initialised; single crate workspace.
- [x] Deps: `patches-sdk = "0.7"` (crates.io) + `patches-dsp` (git
      rev `95c4f29…` of the main `patches` repo until a v0.7.2+
      tag exists).
- [x] `cargo build`, `cargo test`, `cargo clippy` green (99 tests
      pass).
- [x] cdylib build produces a loadable bundle.
- [-] CI scaffolded; release build of cdylib uploaded as a release
      artefact (Linux + macOS at minimum). **Deferred** — tracked
      bundle-repo-side in `patches-bundles`, not blocking this
      ticket's close.
- [-] `v0.7.0` tagged. **Deferred** — tracked bundle-repo-side.
- [x] Main repo: remove patches-vintage workspace member; host
      consumes the cdylib artefact via PluginScanner search path.
- [x] Main repo `just push` green.

## Resolution

- Initial cut into `github.com/Vulpus-Labs/patches-vintage`;
  subsequently consolidated alongside drums + fft into
  `github.com/Vulpus-Labs/patches-bundles` as the
  `patches-vintage/` member of a four-crate Cargo workspace. The
  dedicated single-bundle repo is being retired.
- Main repo: `patches-vintage/` dir removed, workspace member
  dropped from `Cargo.toml`. Bundle-coupled main-repo tests
  (`vintage_baseline`, `vintage_synth_check`,
  `vintage_bundle_scanner`) deleted. `soak_randomised_params`
  rewritten to use `test-plugins/gain` as its FFI subject.
- Bundle-using examples (`poly_synth`, `soft_pad`,
  `fdn_reverb_synth`) moved to
  `patches-bundles/patches-vintage/examples/`.
- CI / `v0.7.0` tag / release artefact remain user-side actions in
  the patches-bundles repo.

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
