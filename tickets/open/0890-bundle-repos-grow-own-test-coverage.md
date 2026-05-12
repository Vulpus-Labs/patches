---
id: "0890"
title: Bundle repos grow their own test coverage
priority: low
created: 2026-05-12
---

## Summary

Phase B of E146 cut `patches-vintage`, `patches-drums`, and
`patches-fft` into their own repos and stripped bundle-coupled tests
out of the main repo. Some of those deleted tests had module-side
coverage value that should be reconstituted in the bundle repos.

Deleted from main on the bundle cut:

- `patches-integration-tests/tests/vintage_baseline.rs` —
  fixed-seed golden-audio render through `VChorus`; verified
  bit-for-bit stable output via SHA-256 against
  `fixtures/vintage_baseline.{patches,f32,sha256}`.
- `patches-integration-tests/tests/vintage_synth_check.rs` —
  compile-check of `vintage_synth.patches` (now at
  `patches-vintage/examples/vintage_synth.patches`).
- `patches-integration-tests/tests/vintage_bundle_scanner.rs` —
  PluginScanner loads the cdylib + smoke-processes a VChorus
  instance to prove the audio-thread path is live.
- The `convolution_reverb_ir` enum-round-trip case from
  `enum_resolution.rs` and the IR-decode-on-prepare assertion from
  `structural_pipeline.rs`.

## Acceptance criteria

- [ ] `patches-vintage` grows a golden-audio regression test
      analogous to `vintage_baseline` — render
      `examples/vintage_synth.patches` through the in-repo rlib
      path, hash the buffer, assert byte-for-byte stable. Pin
      golden bytes + SHA-256 in the repo.
- [ ] `patches-vintage` grows a self-test that loads its own
      cdylib via PluginScanner and asserts every exported module
      registers + can be instantiated. (The main-repo equivalent
      lived in `vintage_bundle_scanner.rs`.)
- [ ] `patches-drums` grows analogous golden-audio + scanner
      self-tests.
- [ ] `patches-fft` grows:
  - a structural-param IR-decode-on-prepare test for
    `ConvolutionReverb` (the old `structural_pipeline` test's
    IR-specific assertion);
  - an enum-round-trip test for `ConvReverb`'s `ir` enum (the
    old `convolution_reverb_ir` test).
- [ ] Each bundle repo's `cargo test` covers the module surface
      without depending on main-repo host crates beyond what's
      already in its `dev-dependencies`.

## Notes

The bundle repos currently dev-dep `patches-sdk` only. Module-side
golden tests need `patches-dsp` (already a git dep) plus
`patches-core` (transitively available via patches-sdk). Scanner
self-tests need `patches-ffi` from main repo via git — adding it
as a dev-dep mirrors how main repo currently tests test-plugins.

This ticket is intentionally low priority: the main host's coverage
of FFI + structural pipeline + enum round-trip survives via
`test-plugins/*` substitution; the deleted tests were *module-side*
quality gates that bundle authors should own anyway.
