---
id: "0874"
title: Extract pitch_shift + convolution_reverb into patches-fft (harness + bundle)
priority: medium
created: 2026-05-11
---

## Summary

Create a `patches-fft/` directory containing two workspace crates:

- `patches-fft-harness` (rlib): holds the FFT buffering harness moved
  out of `patches-dsp` — `slot_deck`, `window_buffer`,
  `partitioned_convolution`, `spectral_pitch_shift`.
- `patches-fft-bundle` (cdylib + rlib): holds the pitch_shift and
  convolution_reverb modules moved out of `patches-modules`. Single
  `export_modules!` invocation.

`RealPackedFft` stays in `patches-dsp` — observation (spectrum
analyser), vintage vdco tests, integration-tests, and modules
test_support all depend on the transform itself.

In-monorepo step toward the repo cut in
[E146](../../epics/open/E146-monorepo-split.md).

## Acceptance criteria

- [x] `patches-fft-harness` crate exists with:
  - [x] `src/slot_deck/` moved from `patches-dsp/src/slot_deck/`
  - [x] `src/window_buffer.rs` moved from `patches-dsp/src/window_buffer.rs`
  - [x] `src/partitioned_convolution/` moved from `patches-dsp/src/partitioned_convolution/`
  - [x] `src/spectral_pitch_shift/` moved from `patches-dsp/src/spectral_pitch_shift/`
  - [x] depends on `patches-dsp` (for `RealPackedFft`) and `patches-core`
- [x] `patches-fft-bundle` crate exists with:
  - [x] `crate-type = ["cdylib", "rlib"]`
  - [x] `src/pitch_shift.rs` moved from `patches-modules/src/pitch_shift.rs`
  - [x] `src/convolution_reverb/` moved from `patches-modules/src/convolution_reverb/`
  - [x] depends on `patches-fft-harness`, `patches-dsp`,
        `patches-core`, `patches-ffi-common`, `patches-io`, `rtrb`
  - [x] single `export_modules!` invocation listing PitchShift +
        ConvolutionReverb + StereoConvReverb (3 entries; stereo is a
        separate type in tree)
  - [x] in-process `pub fn register(r: &mut patches_core::registry::Registry)`
        for transition
- [x] Tests migrated:
  - [x] `patches-dsp/tests/slot_deck/*` → `patches-fft-harness/tests/slot_deck/*`
  - [x] `patches-dsp/tests/fft_lowpass.rs` → `patches-fft-harness/tests/fft_lowpass.rs`
  - [x] `patches-modules/src/convolution_reverb/tests.rs` travels with the module
- [x] Re-exports of moved types deleted from
      [patches-dsp/src/lib.rs](../../patches-dsp/src/lib.rs)
      (slot_deck, WindowBuffer, partitioned_convolution,
      SpectralPitchShifter).
- [x] `cargo test -p patches-fft-harness`, `cargo test -p patches-fft-bundle`,
      `cargo test -p patches-dsp`, `cargo test -p patches-modules` all
      green (24 + 3 + 223 + 319 tests respectively).
- [x] `cargo build -p patches-fft-bundle` produces a loadable cdylib.
- [x] Forbidden-edge lint passes; pitch_shift / convolution_reverb
      no longer in patches-modules dep closure.

## Notes

Why two crates, not one:

- Third-party FFT-based module authors can depend on
  `patches-fft-harness` alone (OLA/WOLA scaffolding) without pulling
  the stdlib bundle.
- Bundle stays narrow: trait impls, parameter wiring, descriptors.

Module deps audit (current):

- pitch_shift uses: `RealPackedFft`, `WindowBuffer`, `OverlapBuffer`,
  `SlotDeckConfig`, `SpectralPitchShifter`, `AtomicF32`
- convolution_reverb uses: `NonUniformConvolver`, `OverlapBuffer`,
  `SlotDeckConfig`, `AtomicF32`, `xorshift64`

`AtomicF32` and `xorshift64` are general dsp utilities — stay in
patches-dsp. `ir_loader.rs` reads WAV-format impulse responses via
`patches-io` — stays inside the bundle (module-policy, not harness).

Removal from `default_registry()` + PluginScanner wiring is ticket 0876.

## Implementation notes

- `patches-fft-harness` carries a small local `test_support` module
  (`assert_within!` + `dominant_bin`) so the migrated tests do not
  need to reach into `patches-dsp::test_support`.
- The fft bundle types in 0874 leave `default_registry()` (mirrors
  the drum-extraction approach from 0873): in-tree tests that need
  `ConvolutionReverb` downcast access (`enum_resolution`,
  `structural_pipeline`) now pull `patches-fft-bundle` as an
  rlib dev-dep and call `patches_fft_bundle::register`.
  `alloc_trap::registry_with_bundles` was extended to scan the
  `patches-fft-bundle` cdylib alongside vintage and drums.
- `rtrb = "0.3"` added as a direct dep of both new crates: slot_deck
  and ir_loader use it for lock-free producer/consumer channels.
- `patches-manifest/data/module-manifest.json` regenerated.
