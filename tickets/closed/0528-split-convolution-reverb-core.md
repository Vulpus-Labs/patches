---
id: "0528"
title: Extract ConvReverbCore and inline tests from convolution_reverb
priority: low
created: 2026-04-17
epic: E090
---

## Summary

After 0510 split out `ir_loader.rs`, `params.rs`, and `stereo.rs`, the
remaining [patches-modules/src/convolution_reverb/mod.rs](../../patches-modules/src/convolution_reverb/mod.rs)
is 577 lines. The bulk is `ConvReverbCore` (226 lines at `91–354`), the
mono `ConvolutionReverb` wrapper with its `Module` / `FileProcessor`
/ `PeriodicUpdate` impls, and a ~100-line inline `mod tests` block.

`ConvReverbCore` is shared between mono and stereo paths already, so
it deserves its own sibling file.

## Acceptance criteria

- [ ] New sibling `core.rs` containing `ConvReverbCore` and its
      `Drop` + `impl` blocks; mono `ConvolutionReverb` stays in
      `mod.rs`.
- [ ] Inline `mod tests` moved to sibling `tests.rs`.
- [ ] `convolution_reverb/mod.rs` under ~300 lines.
- [ ] Module registrations and crate-external surface unchanged.
- [ ] `cargo build -p patches-modules`, `cargo test -p patches-modules`,
      `cargo clippy` clean.
- [ ] Audio-thread invariants preserved (no new allocations or locks
      in the process path).

## Notes

E090. Continuation of 0510 (E086). No behaviour change.
