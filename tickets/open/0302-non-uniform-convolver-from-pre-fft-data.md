---
id: "0302"
title: "patches-dsp: NonUniformConvolver from pre-FFT'd data"
priority: medium
created: 2026-04-11
---

## Summary

Add a constructor to `NonUniformConvolver` (and `IrPartitions`) that
accepts pre-computed frequency-domain partition data, skipping the forward
FFT step. This enables ConvolutionReverb's `process_file` to return FFT'd
data that the module can use directly at plan adoption time.

## Acceptance criteria

- [ ] `IrPartitions` gains a `from_packed(partitions: Vec<Box<[f32]>>, block_size: usize)` constructor (or equivalent) that wraps pre-FFT'd partition data without re-transforming
- [ ] `NonUniformConvolver` gains a `from_pre_fft(data: &[f32], base_block_size: usize, max_tier_block_size: usize)` constructor that deserializes the tier structure from a flat `&[f32]` layout
- [ ] A corresponding `to_packed_vec(&self) -> Vec<f32>` (or similar) serialization method produces the flat layout consumed by `from_pre_fft`
- [ ] Round-trip test: `NonUniformConvolver::new(ir)` → `to_packed_vec()` → `from_pre_fft()` produces identical convolution output
- [ ] The flat layout includes a small header (tier count, partition counts and block sizes per tier) so the deserializer can reconstruct the tier hierarchy
- [ ] `cargo test -p patches-dsp` passes
- [ ] `cargo clippy -p patches-dsp` clean

## Notes

The serialization format is a private contract between `process_file`
(which calls `to_packed_vec`) and the module's `update_validated_parameters`
(which calls `from_pre_fft`). It does not need to be stable across
versions — both sides are compiled together.

The header should be minimal: a few `f32`-encoded integers at the start of
the `Vec<f32>`. Using `f32` for header values avoids alignment concerns and
keeps the entire buffer homogeneous.

Epic: E056
ADR: 0028
