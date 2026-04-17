---
id: "0531"
title: Split patches-dsp partitioned_convolution/mod.rs by stage
priority: low
created: 2026-04-17
epic: E090
---

## Summary

[patches-dsp/src/partitioned_convolution/mod.rs](../../patches-dsp/src/partitioned_convolution/mod.rs)
is 555 lines with four clearly separable stages:
`complex_multiply_accumulate_packed` / `complex_multiply_packed` (SIMD
frequency-domain ops), `IrPartitions` (IR preparation),
`PartitionedConvolver` (uniform-partition convolver), and
`ConvolutionTier` + `NonUniformConvolver` (non-uniform partitioning).
A sibling `tests.rs` already exists.

## Acceptance criteria

- [ ] Convert to `partitioned_convolution/mod.rs` + sibling submodules:
      `complex.rs` (complex_multiply_* functions),
      `ir_partitions.rs` (IrPartitions),
      `convolver.rs` (PartitionedConvolver),
      `non_uniform.rs` (ConvolutionTier + NonUniformConvolver).
- [ ] Public re-exports from `patches-dsp/src/lib.rs` unchanged.
- [ ] `mod.rs` under ~80 lines (module declarations + re-exports).
- [ ] Audio-thread invariants preserved (partitioned convolution is
      hot-path — no new allocations or virtual calls introduced).
- [ ] `cargo build -p patches-dsp`, `cargo test -p patches-dsp`,
      `cargo clippy` clean.

## Notes

E090. No behaviour change. Benchmarks in `patches-modules/examples/`
should be unaffected.
