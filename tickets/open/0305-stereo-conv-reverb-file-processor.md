---
id: "0305"
title: "patches-modules: StereoConvReverb FileProcessor impl"
priority: medium
created: 2026-04-11
---

## Summary

Implement `FileProcessor` for `StereoConvReverb`, mirroring the
`ConvolutionReverb` implementation but handling stereo IR files (two
channels of FFT partition data).

## Acceptance criteria

- [ ] `StereoConvReverb` implements `FileProcessor`
- [ ] `process_file` reads a stereo audio file, resamples both channels, partitions and FFTs both, and packs both channels' data into a single `Vec<f32>`
- [ ] For mono IR files loaded into StereoConvReverb, the single channel is used for both L and R (same behaviour as current `resolve_stereo_ir`)
- [ ] `update_validated_parameters` unpacks the stereo data and reconstructs both L and R convolvers
- [ ] The `"path"` string parameter is replaced with a `File` parameter
- [ ] Existing patches using stereo convolution reverb continue to work with updated syntax
- [ ] `cargo test -p patches-modules` passes
- [ ] `cargo clippy -p patches-modules` clean

## Notes

The packed format for stereo is a private contract: e.g. a header
indicating channel count, followed by L channel partition data, followed by
R channel partition data. Both channels share the same tier structure since
they come from the same IR file at the same sample rate.

Epic: E056
ADR: 0028
Depends: 0303
