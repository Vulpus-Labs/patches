---
id: "0873"
title: Extract drums into patches-drums workspace crate
priority: medium
created: 2026-05-11
---

## Summary

Move the seven drum modules (kick, snare, hihat, cymbal, tom,
clap_drum, claves) out of `patches-modules` and into a new
`patches-drums` workspace crate built as `cdylib + rlib`. Move the
`patches-dsp::drum::*` submodule (DecayEnvelope, PitchSweep,
MetallicTone, BurstGenerator, saturate) into the new crate as
`primitives/`, since drum modules are its only consumers.

In-monorepo step toward the repo cut in
[E146](../../epics/open/E146-monorepo-split.md) /
[ADR 0073](../../adr/0073-monorepo-split-into-successor-repos.md).

## Acceptance criteria

- [x] `patches-drums` crate exists with `crate-type = ["cdylib", "rlib"]`.
- [x] Drum modules moved from `patches-modules/src/` into `patches-drums/src/`.
- [x] `patches-dsp/src/drum/` moved to `patches-drums/src/primitives/`.
- [x] `crate::fast_sine` (metallic.rs) and `crate::fast_tanh`
      (saturate.rs) rewritten to `patches_dsp::fast_sine` /
      `patches_dsp::fast_tanh`.
- [x] `pub use drum::{...}` deleted from
      [patches-dsp/src/lib.rs:86](../../patches-dsp/src/lib.rs#L86).
- [x] `patches_ffi_common::export_modules!` invocation in
      `patches-drums/src/lib.rs` lists all seven drum types
      (eight registered names — hihat exports `ClosedHiHat` and
      `OpenHiHat`).
- [x] In-process `pub fn register(r: &mut patches_core::registry::Registry)`
      provided for transition (mirrors vintage pattern).
- [x] Drum unit tests migrated; `cargo test -p patches-drums` green
      (51 tests).
- [x] `cargo test -p patches-dsp` still green after drum submodule
      removal.
- [x] `cargo build -p patches-drums` produces a loadable cdylib that
      `PluginScanner` can read its descriptor from (manifest
      round-trip covered by `ffi_bundle_tests`; integration loader
      coverage via `audio_tick_no_alloc_drum_machine`).
- [x] Forbidden-edge lint passes (drum modules no longer in
      patches-modules dep set).

## Notes

Pattern reference: [patches-vintage/Cargo.toml](../../patches-vintage/Cargo.toml)
and [patches-vintage/src/lib.rs:67](../../patches-vintage/src/lib.rs#L67)
(`export_modules!` invocation).

Drum modules currently use:

- `patches_core::*` — modules, params, cables, frame, harness
- `patches_dsp::drum::*` — moves with the modules
- `patches_dsp::{MonoPhaseAccumulator, SvfKernel, svf_f, q_to_damp, xorshift64, fast_sine, fast_tanh}` — stay in patches-dsp
- `patches_core::test_support::ModuleHarness` — dev-dep, feature `test-support`

Removal of drum types from `default_registry()` is ticket 0876.
Wiring into `PluginScanner` default path is also 0876. Keep this
ticket focused on the crate boundary.

## Implementation notes

- `patches-modules/src/test_support.rs` was only used by drum unit
  tests. Spectral helpers (`magnitude_spectrum`, `freq_to_bin`,
  `band_energy`, `dominant_bin`, `windowed_rms`) plus the
  `assert_within!` macro moved into `patches-drums/src/test_support.rs`;
  the now-unused `patches-modules` module was deleted.
- `kick.rs` previously pulled `crate::common::frequency::C0_FREQ`
  (`16.351_598`) and `crate::common::approximate::fast_exp2`. The
  constant is inlined at the top of the new file; the function is
  imported as `patches_dsp::fast_exp2` directly.
- Drum registrations are removed from `patches_modules::default_registry()`
  in this ticket (0876 keeps the PluginScanner wiring; the registry
  surface was simpler to drop now than to add a transient
  patches-drums dep to patches-modules). `patches-manifest/data/module-manifest.json`
  regenerated to match.
- `patches-integration-tests::alloc_trap::registry_with_vintage`
  renamed to `registry_with_bundles` and now scans both the vintage
  and drum dylibs so `audio_tick_no_alloc_drum_machine` keeps
  passing.
