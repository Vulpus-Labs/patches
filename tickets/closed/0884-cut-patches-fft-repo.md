---
id: "0884"
title: Cut patches-fft repo (harness + bundle)
priority: medium
created: 2026-05-11
---

## Summary

Move patches-fft-harness and patches-fft-bundle (extracted in-monorepo
by ticket 0874) into a single repo as a two-crate Cargo workspace.

Depends on ticket 0888 (patches-sdk + patches-core + patches-ffi-common
published to crates.io). Sibling to 0882 (vintage cut) and 0883 (drums
cut); can run in parallel after 0888.

## Acceptance criteria

- [x] New repo `patches-fft` initialised with two-crate Cargo
      workspace:
  - [x] `patches-fft-harness/` (rlib)
  - [x] `patches-fft-bundle/` (cdylib + rlib)
- [x] patches-fft-harness deps:
  - [x] `patches-dsp` (git rev `95c4f29…` of main `patches` repo) —
        for RealPackedFft
  - [x] `patches-core = "0.7"` (crates.io)
- [x] patches-fft-bundle deps:
  - [x] `patches-sdk = "0.7"` (crates.io)
  - [x] `patches-dsp` (git rev `95c4f29…` of main `patches` repo)
  - [x] `patches-fft-harness` (path-dep inside this repo)
  - [x] `patches-io` (git rev `95c4f29…` of main `patches` repo) —
        needed by IR loader; not on crates.io per ADR 0073.
- [x] `cargo build --workspace`, `cargo test --workspace`,
      `cargo clippy --workspace` green (59 tests pass).
- [x] Bundle cdylib produced.
- [-] CI scaffolded; release build uploaded as artefact.
      **Deferred** — tracked bundle-repo-side in `patches-bundles`.
- [-] `v0.7.0` tagged. **Deferred** — tracked bundle-repo-side.
- [x] Main repo: remove both crate members; host consumes the
      bundle cdylib via PluginScanner search path.
- [x] Main repo `just push` green.

## Resolution

- Initial cut into `github.com/Vulpus-Labs/patches-fft` as a
  two-crate workspace (harness + bundle); patches-io added as a git
  dep on the bundle since the IR loader uses it. Subsequently
  consolidated alongside vintage + drums into
  `github.com/Vulpus-Labs/patches-bundles`, where harness and bundle
  sit as two of four workspace members. The dedicated patches-fft
  repo is being retired.
- Main repo: `patches-fft/` dir removed, both workspace members
  dropped from `Cargo.toml`. Bundle-coupled main-repo tests:
  `structural_pipeline` rewritten to use `test-plugins/structural-
  string`; IR-decode-on-prepare assertion dropped (was
  ConvolutionReverb-specific). `enum_resolution`'s
  `convolution_reverb_ir` test deleted; `patches_fft_bundle::
  register` call removed from registry init.
- Bundle-using examples (`pitch_shift_fifth`, `pad`) moved to
  `patches-bundles/patches-fft-bundle/examples/`. `shimmer.patches`
  deleted from main (mixed vintage+fft — doesn't fit one bundle).
- CI / `v0.7.0` tag / release artefact remain user-side actions in
  the patches-bundles repo.

## Notes

Why both crates live in one repo: tight coupling between bundle and
harness during initial iteration. If a third-party FFT-based module
crate emerges, patches-fft-harness can be published separately and
the bundle continues to consume it.

`RealPackedFft` is in patches-dsp (foundation) — not in harness.
Harness consumes it. Tests that need RealPackedFft (fft_lowpass.rs,
slot_deck/support.rs) pull patches-dsp as dev-dep.

Out of scope:

- Splitting harness into a separate repo. Stays here for now.
- Optimising IR loader (lives in bundle, depends on patches-io from
  foundation).
