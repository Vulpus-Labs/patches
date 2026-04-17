---
id: "0510"
title: Split patches-modules convolution_reverb.rs
priority: medium
created: 2026-04-16
---

## Summary

`patches-modules/src/convolution_reverb.rs` is 1139 lines covering
mono `ConvolutionReverb`, stereo `StereoConvReverb`, the
`IrLoader` background thread, shared parameter plumbing, and IR
resolution.

## Acceptance criteria

- [ ] Convert to `convolution_reverb/mod.rs` with submodules:
      `ir_loader.rs` (background thread + request/response
      plumbing), `params.rs` (SharedParams + IR resolution), and
      `stereo.rs` (StereoConvReverb). Mono stays in `mod.rs`.
- [ ] Module registrations/exports unchanged.
- [ ] `cargo build -p patches-modules`, `cargo test -p patches-modules`,
      `cargo clippy` clean.
- [ ] Audio-thread safety invariants preserved (no new allocations
      or locks in the process path).

## Notes

E086. No behaviour change.
