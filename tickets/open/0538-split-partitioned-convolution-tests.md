---
id: "0538"
title: Split patches-dsp partitioned_convolution/tests.rs by subject
priority: low
created: 2026-04-17
epic: E090
---

## Summary

[patches-dsp/src/partitioned_convolution/tests.rs](../../patches-dsp/src/partitioned_convolution/tests.rs)
is 676 lines and already carries clear section banners for its four
subjects: `complex_multiply_accumulate_packed`, `IrPartitions`,
`PartitionedConvolver`, `NonUniformConvolver`.

## Acceptance criteria

- [ ] Convert to stub `src/partitioned_convolution/tests.rs` declaring
      a submodule tree under `src/partitioned_convolution/tests/`.
- [ ] Category split aligned with the existing banners and the impl
      layout from ticket 0531:
      - `complex.rs` — `complex_multiply_*` tests
      - `ir_partitions.rs` — IrPartitions tests
      - `convolver.rs` — PartitionedConvolver tests
      - `non_uniform.rs` — NonUniformConvolver tests
- [ ] Shared helpers (if any) in `tests/mod.rs` or `tests/support.rs`.
- [ ] `cargo test -p patches-dsp` passes with the same test count.
- [ ] `cargo build -p patches-dsp`, `cargo clippy` clean.

## Notes

E090. Can land together with 0531 (same subject axes). No test logic
edits.
