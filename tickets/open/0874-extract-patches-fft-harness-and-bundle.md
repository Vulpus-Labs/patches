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

- [ ] `patches-fft-harness` crate exists with:
  - [ ] `src/slot_deck/` moved from `patches-dsp/src/slot_deck/`
  - [ ] `src/window_buffer.rs` moved from `patches-dsp/src/window_buffer.rs`
  - [ ] `src/partitioned_convolution/` moved from `patches-dsp/src/partitioned_convolution/`
  - [ ] `src/spectral_pitch_shift/` moved from `patches-dsp/src/spectral_pitch_shift/`
  - [ ] depends on `patches-dsp` (for `RealPackedFft`) and `patches-core`
- [ ] `patches-fft-bundle` crate exists with:
  - [ ] `crate-type = ["cdylib", "rlib"]`
  - [ ] `src/pitch_shift.rs` moved from `patches-modules/src/pitch_shift.rs`
  - [ ] `src/convolution_reverb/` moved from `patches-modules/src/convolution_reverb/`
  - [ ] depends on `patches-fft-harness`, `patches-dsp` (RealPackedFft + AtomicF32 + xorshift64), `patches-core`, `patches-registry`, `patches-ffi-common`
  - [ ] single `export_modules!` invocation listing PitchShift + ConvolutionReverb (+ stereo variant if separate type)
  - [ ] in-process `pub fn register(r: &mut patches_registry::Registry)` for transition
- [ ] Tests migrated:
  - [ ] `patches-dsp/tests/slot_deck/*` → `patches-fft-harness/tests/slot_deck/*`
  - [ ] `patches-dsp/tests/fft_lowpass.rs` → `patches-fft-harness/tests/fft_lowpass.rs`
  - [ ] `patches-modules/src/convolution_reverb/tests.rs` travels with the module
- [ ] Re-exports of moved types deleted from
      [patches-dsp/src/lib.rs](../../patches-dsp/src/lib.rs)
      (slot_deck, WindowBuffer, partitioned_convolution,
      SpectralPitchShifter).
- [ ] `cargo test -p patches-fft-harness`, `cargo test -p patches-fft-bundle`,
      `cargo test -p patches-dsp`, `cargo test -p patches-modules` all green.
- [ ] `cargo build -p patches-fft-bundle` produces a loadable cdylib.
- [ ] Forbidden-edge lint updated: pitch_shift / convolution_reverb
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
