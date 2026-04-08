---
id: "0198"
title: Integration tests for periodic_update_interval scaling at 2× oversampling
priority: medium
created: 2026-03-26
epic: E036
depends-on: "0197"
---

## Summary

Add integration tests that verify coefficient-ramp behaviour is correct when `periodic_update_interval` differs from the base value of 32. The key invariant is that after `periodic_update()` fires (having called `begin_ramp(64)`), the filter reaches its target coefficients in exactly 64 samples — not 32.

## Acceptance criteria

- [ ] A new test file (or extension of an existing one) in `patches-integration-tests/tests/` contains at least the following tests:
  - **`ramp_completes_in_one_interval_at_1x`** — `AudioEnvironment` with `periodic_update_interval = 32`; drive a filter from one cutoff to another; assert the coefficient ramp is settled after exactly 32 ticks and not before.
  - **`ramp_completes_in_one_interval_at_2x`** — same test with `periodic_update_interval = 64`; ramp should be settled after 64 ticks.
  - **`periodic_fires_at_correct_cadence_at_2x`** — with `periodic_update_interval = 64`, verify that `periodic_update()` is called once every 64 samples (not every 32).
- [ ] Tests use `HeadlessEngine` (or a local test module/harness) without audio hardware.
- [ ] `cargo test -p patches-integration-tests` passes with zero failures.
- [ ] `cargo clippy` passes with zero warnings.

## Notes

A local test module recording `periodic_update` call counts (similar to the `Probe` module in `connectivity_notification.rs`) is the cleanest approach. Use `Arc<AtomicU32>` to share state without allocation on the audio thread (count is written from the "audio thread" context; read from the test thread only after the engine is stopped).
