---
id: "0177"
title: Register `FdnReverb`, integration tests, example patch
priority: medium
created: 2026-03-22
epic: "E030"
depends_on: ["0176"]
---

## Summary

Registers `FdnReverb` in the default module registry, adds an integration test
confirming the reverb tail decays correctly under the full engine, and adds an
example `.patches` file demonstrating stereo reverb on a polyphonic synth.

## Acceptance criteria

- [ ] `FdnReverb` registered in `patches-modules/src/lib.rs` under the name
      `"FdnReverb"`.
- [ ] Integration test in `patches-integration-tests/tests/fdn_reverb.rs`:
  - Constructs a minimal patch: `Oscillator → FdnReverb → AudioOut` using
    `HeadlessEngine`.
  - Runs for enough ticks to fill the delay lines, then silences the input.
  - Asserts that output is non-zero during the tail and decays to below a
    noise floor within 1.5× the expected RT60 at the default `hall` character
    and `size = 0.5`.
- [ ] `examples/fdn_reverb_synth.patches` — a runnable patch demonstrating
      `FdnReverb` in stereo: a poly oscillator summed to mono feeding an
      `FdnReverb` with `character: hall`, with both `out_l` and `out_r`
      connected to a stereo audio output.
- [ ] `cargo clippy`, `cargo test` pass with no new warnings.

## Notes

The integration test does not require audio hardware; `HeadlessEngine` runs the
full plan-swap and tick sequence without CPAL.

The example patch should use enough voices to demonstrate the reverb tail
clearly (e.g. a short arpeggio driven by `ClockSequencer`) but remain simple
enough to be readable as documentation.
