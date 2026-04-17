---
id: "0545"
title: Direct-convolution reference cross-check for partitioned convolver
priority: low
created: 2026-04-17
epic: E092
depends_on: ["0531", "0538"]
---

## Summary

[patches-dsp/src/partitioned_convolution/tests.rs](../../patches-dsp/src/partitioned_convolution/tests.rs)
has 26 `#[test]` functions covering complex multiply-accumulate, IR
preparation, and both uniform and non-uniform convolver construction
and step. It does not cross-check the output against a direct
convolution for a known IR. Add that check.

Depends on 0531 (impl split) and 0538 (tests split) so the new test
lands into the `convolver.rs` category file per 0538.

## Acceptance criteria

- [ ] New test in the `convolver` category (post-0538 split) that:
      - Constructs a short impulse response (≤64 taps) with known
        content — a specific choice is fine; a Hann-windowed
        random-phase IR, or a simple first-order filter response,
        or even a pair of impulses. Document which.
      - Constructs a `PartitionedConvolver` for that IR.
      - Drives the convolver with an input signal of at least
        3× the partition length (so the input crosses at least one
        partition boundary).
      - Computes the reference output by direct convolution in
        `f64` and asserts per-sample error stays below a defined
        bound (suggestion: `1e-5` max abs diff, but tune to the
        FFT precision characteristics — the existing tests'
        tolerance conventions are the starting point).
- [ ] **Latency invariant.** The test documents and asserts the
      convolver's claimed latency — output sample at position `k`
      of the direct-convolution reference shows up at position
      `k + latency` in the partitioned convolver's output, not
      earlier. Any nonzero samples in positions `0..latency` of
      the partitioned output fail the test.
- [ ] Optional second test covering `NonUniformConvolver` with the
      same direct-convolution reference, to confirm the non-
      uniform path produces equivalent output (subject to its
      own latency).
- [ ] `cargo test -p patches-dsp` clean.

## Notes

This is the single biggest behavioural gap in
`partitioned_convolution/tests.rs`: the existing tests verify that
the pieces wire up and step without panicking, but nothing verifies
that the output is actually the convolution of input with IR. A
regression in complex multiply-accumulate ordering, partition
indexing, or IR pre-processing could pass every existing test and
still produce wrong output.

`f64` reference is sensible because the partitioned path does `f32`
FFT and we want the reference-to-candidate comparison to reflect
algorithmic error, not accumulated `f32` rounding in the reference.

If 0531 / 0538 are not landed when this ticket runs, add the test
to the current `tests.rs` and note that file placement may shift
post-split. Do not wait indefinitely.

No behavioural change. Pure test addition.
