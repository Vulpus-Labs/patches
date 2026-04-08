---
id: "0218"
title: Add T9 golden-file test for FDN reverb
priority: low
created: 2026-03-30
---

## Summary

The FDN reverb (`patches-modules/src/fdn_reverb.rs`) is a complex algorithm
(8 Thiran-interpolated delay lines, Hadamard mixing, biquad absorption, LFO
modulation) whose output is impractical to verify analytically. A T9 golden-
file test would compare the output of a known input (e.g. an impulse or short
sweep) against a stored reference, providing regression protection against
unintended changes.

## Acceptance criteria

- [ ] A golden-file reference is generated from the current implementation at a
  fixed sample rate (48 kHz) with fixed parameters and stored under
  `patches-integration-tests/golden/fdn_reverb_impulse.bin` (or similar).
- [ ] An integration test reads the golden file and verifies that the current
  output matches within a tolerance (e.g. max absolute difference < 1e-4).
- [ ] The test is documented with the reference version, sample rate, and
  parameter values used to generate the golden file.
- [ ] A README or comment in the golden-file directory explains how to
  regenerate the file if the algorithm is intentionally changed.
- [ ] `cargo test -p patches-integration-tests` passes, `cargo clippy` clean.

## Notes

Golden files must be committed to the repository. Use a compact binary format
(little-endian f32 samples) to keep file sizes small. The regeneration
procedure should be a simple `cargo run --example generate_golden` or similar.

Consider whether this ticket should be deferred until after T-0213–T-0215 are
complete, as the biquad kernel used inside `fdn_reverb.rs` has already moved
to `patches-dsp` (E039), meaning the golden output should be stable.

ADR 0022 technique reference: **T9** — golden-file / reference comparison.
