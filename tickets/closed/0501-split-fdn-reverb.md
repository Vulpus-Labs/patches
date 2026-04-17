---
id: "0501"
title: Split patches-modules fdn_reverb.rs
priority: medium
created: 2026-04-16
---

## Summary

`patches-modules/src/fdn_reverb.rs` is 760 lines covering the 8-line
FDN body, per-line absorption and Thiran all-pass interpolation,
Hadamard feedback matrix, and parameter handling for the stereo
`FdnReverb` module.

## Acceptance criteria

- [ ] Convert to `fdn_reverb/mod.rs` with submodules:
      `line.rs` (per-line delay + high-shelf absorption + Thiran
      AP), `matrix.rs` (Hadamard feedback mixing), `params.rs`
      (size / brightness / pre-delay / mix plumbing and CV blend).
- [ ] Module impl (`Module for FdnReverb`) stays in `mod.rs`.
- [ ] No new allocations in the process path.
- [ ] `cargo build -p patches-modules`, `cargo test -p patches-modules`,
      `cargo clippy` clean.

## Notes

E086. Confirm exact submodule boundaries when opening the file; the
shape above is the intended target.
