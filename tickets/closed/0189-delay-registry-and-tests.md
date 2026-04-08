---
id: "0189"
title: Register Delay and StereoDelay; integration tests
epic: "E034"
priority: medium
created: 2026-03-24
---

## Summary

Register `Delay` and `StereoDelay` in the module registry so they are available
to the DSL pipeline, then write integration tests that verify end-to-end behaviour
through the engine using `HeadlessEngine`.

## Acceptance criteria

### Registry

- [ ] Both modules registered in the module registry (wherever other modules such
  as `FdnReverb` are registered)
- [ ] Both modules constructable from DSL with `ModuleShape::length` driving tap
  count, e.g. `Delay { length: 3 }` and `StereoDelay { length: 2 }`

### Integration tests (`patches-integration-tests/tests/delay_modules.rs`)

- [ ] **Delay time accuracy** — single impulse into a 1-tap `Delay` at `delay_ms = N`;
  verify the impulse appears at output sample offset ⌊N × sample_rate / 1000⌋ (±1 sample)
- [ ] **Feedback decay** — with `feedback = 0.5` and `drive = 1.0`, verify each
  successive repeat is attenuated relative to the previous one (not growing);
  run for at least 5 × delay period
- [ ] **Feedback saturation bound** — with `feedback = 1.0`, `drive = 10.0`, and a
  sustained sine input, verify output stays within (−2.0, 2.0) indefinitely
  (tanh bounds the feedback loop)
- [ ] **Send/return path** — connect a second signal to `return/0`; verify it
  appears in the output (i.e., it is added before gain and contributes to wet mix)
  but does not appear at `send/0` (send is pre-return)
- [ ] **Tone rolloff** — with `tone = 0.0` and repeated feedback, verify that
  a 10 kHz test tone decays faster than a 100 Hz tone over the same number of repeats
- [ ] **dry_wet = 0** — output equals input exactly (no wet bleed)
- [ ] **dry_wet = 1** — dry signal absent from output (pure wet)
- [ ] **StereoDelay pingpong routing** — feed an impulse into `in_l` only with
  `pingpong/0 = true`; after one delay period the signal should appear on `out_r`;
  after two periods it should appear on `out_l`
- [ ] **StereoDelay pan** — feed the same signal into both channels; at `pan = 1.0`
  verify `out_l` ≈ 0 and `out_r > 0`; at `pan = −1.0` verify `out_r` ≈ 0 and `out_l > 0`
- [ ] **Zero taps** — `Delay { length: 0 }` with `dry_wet = 0` passes signal
  through unmodified; no panic

### General

- [ ] `cargo clippy` clean across all crates
- [ ] `cargo test` passes (all existing tests unaffected)

## Notes

Use the same `HeadlessEngine` helper from `patches-integration-tests/src/lib.rs`
used in the mixer and reverb tests. No real audio hardware required; all tests
run in CI without `#[ignore]`.

For timing-sensitive tests (delay time accuracy), prefer asserting a sample-index
range of ±1 rather than exact equality to avoid sensitivity to rounding in the
`delay_ms → samples` conversion.
