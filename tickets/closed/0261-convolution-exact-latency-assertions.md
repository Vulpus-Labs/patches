---
id: "0261"
title: Partitioned convolution exact latency assertions
priority: low
created: 2026-04-02
---

## Summary

The partitioned convolution latency tests (`latency_first_nonzero_output` and
`latency_delayed_ir_offset`) check that output appears and that delayed IRs
produce output at roughly the right time, but they do not assert the *exact*
sample offset. For `PartitionedConvolver` the latency should be exactly 0
samples (overlap-save produces the first valid block immediately). For
`NonUniformConvolver` the latency should equal `base_block_size`. A latency
regression would cause audible time-alignment errors in convolution reverb.

## Acceptance criteria

- [ ] For `PartitionedConvolver` with a unit impulse IR: the first output sample
      of the first block equals the first input sample (within 1e-3). Assert that
      output is *not* delayed by one block.
- [ ] For `NonUniformConvolver` with a unit impulse IR: assert that the first
      non-zero output appears at exactly sample index `base_block_size` (or
      document the actual latency if different, and assert that).
- [ ] For `PartitionedConvolver` with a delayed impulse IR (delay = D samples):
      assert the first non-zero output appears at exactly sample D (within the
      block-processing granularity).
- [ ] Tests in `partitioned_convolution.rs` unit test module.

## Notes

The existing `latency_first_nonzero_output` test checks `output[0].abs() > 0.5`
which confirms output exists but not its exact timing. The new tests should use
precise sample-index assertions.
