---
id: "0890"
title: Bundle repo grows its own test coverage
priority: low
created: 2026-05-12
---

## Summary

Phase B of E146 cut vintage, drums, and fft out of the main repo and
stripped bundle-coupled tests in the process. Some of those deleted
tests had module-side coverage value that should be reconstituted in
the `patches-bundles` repo
(`github.com/Vulpus-Labs/patches-bundles`).

Deleted from main on the bundle cut:

- `patches-integration-tests/tests/vintage_baseline.rs` —
  fixed-seed golden-audio render through `VChorus`; verified
  bit-for-bit stable output via SHA-256 against
  `fixtures/vintage_baseline.{patches,f32,sha256}`.
- `patches-integration-tests/tests/vintage_synth_check.rs` —
  compile-check of `vintage_synth.patches` (now at
  `patches-bundles/patches-vintage/examples/vintage_synth.patches`).
- `patches-integration-tests/tests/vintage_bundle_scanner.rs` —
  PluginScanner loads the cdylib + smoke-processes a VChorus
  instance to prove the audio-thread path is live.
- The `convolution_reverb_ir` enum-round-trip case from
  `enum_resolution.rs` and the IR-decode-on-prepare assertion from
  `structural_pipeline.rs`.

## Acceptance criteria

Reconstituted in `patches-bundles`:

- [ ] Golden-audio regression test for vintage modules analogous to
      `vintage_baseline` — render
      `patches-vintage/examples/vintage_synth.patches` through the
      in-repo rlib path, hash the buffer, assert byte-for-byte
      stable. Pin golden bytes + SHA-256 in the repo.
- [ ] Self-test that loads `libpatches_vintage.{dylib,so,dll}` via
      `PluginScanner` and asserts every exported module registers +
      can be instantiated.
- [ ] Analogous golden-audio + scanner self-tests for `patches-drums`.
- [ ] For `patches-fft-bundle`:
  - a structural-param IR-decode-on-prepare test for
    `ConvolutionReverb` (the old `structural_pipeline` test's
    IR-specific assertion);
  - an enum-round-trip test for `ConvReverb`'s `ir` enum (the
    old `convolution_reverb_ir` test).
- [ ] `cargo test --workspace` in `patches-bundles` covers the
      module surface without depending on main-repo host crates
      beyond what's already in its `dev-dependencies`.

## Notes

`patches-bundles` currently dev-deps `patches-sdk` only. Module-side
golden tests need `patches-dsp` (already a workspace git dep) plus
`patches-core` (transitively available via patches-sdk). Scanner
self-tests need `patches-ffi` from the main repo via git — adding it
as a dev-dep mirrors how main repo currently tests test-plugins.

This ticket is intentionally low priority: the main host's coverage
of FFI + structural pipeline + enum round-trip survives via
`test-plugins/*` substitution; the deleted tests were *module-side*
quality gates that bundle authors should own anyway.
