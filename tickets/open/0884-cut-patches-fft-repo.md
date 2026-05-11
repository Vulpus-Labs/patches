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

- [ ] New repo `patches-fft` initialised with two-crate Cargo
      workspace:
  - [ ] `patches-fft-harness/` (rlib)
  - [ ] `patches-fft-bundle/` (cdylib + rlib)
- [ ] patches-fft-harness deps:
  - [ ] `patches-dsp` (git tag from main `patches` repo) — for RealPackedFft
  - [ ] `patches-core = "0.7"` (crates.io)
- [ ] patches-fft-bundle deps:
  - [ ] `patches-sdk = "0.7"` (crates.io)
  - [ ] `patches-dsp` (git tag from main `patches` repo)
  - [ ] `patches-fft-harness` (path-dep inside this repo)
- [ ] `cargo build --workspace`, `cargo test --workspace`,
      `cargo clippy --workspace` green.
- [ ] Bundle cdylib loads via PluginScanner; descriptor_hash matches
      ABI v12.
- [ ] CI scaffolded; release build uploaded as artefact.
- [ ] `patches-fft-harness@0.7.0` and `patches-fft-bundle@0.7.0`
      tagged (or single `v0.7.0` repo tag if release-plz tooling
      manages per-crate versions internally).
- [ ] Main repo: remove both crate members; host consumes the
      bundle cdylib via PluginScanner search path.
- [ ] Main repo `just push` green.

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
