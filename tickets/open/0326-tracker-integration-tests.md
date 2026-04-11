---
id: "0326"
title: "Integration tests: tracker sequencer round-trip"
priority: medium
created: 2026-04-11
---

## Summary

End-to-end integration tests verifying the full tracker pipeline: DSL
parsing → interpreter → plan building → audio-thread execution →
correct module outputs.

## Acceptance criteria

- [ ] Test: parse a `.patches` file with patterns, a song, a
      MasterSequencer, and PatternPlayers; build and tick the engine;
      verify trigger/gate/cv outputs match expected step data
- [ ] Test: pattern with slides — verify interpolated cv1/cv2 values
      across a tick
- [ ] Test: pattern with repeats — verify sub-tick trigger timing
- [ ] Test: song with multiple rows — verify pattern switching at row
      boundaries
- [ ] Test: song with `@loop` — verify playback loops to the correct row
- [ ] Test: hot-reload — change a pattern's step data, rebuild plan,
      verify updated data reaches modules
- [ ] Test: transport controls — start/stop/pause/resume via trigger
      inputs
- [ ] Tests live in `patches-integration-tests/tests/tracker.rs`
- [ ] `cargo test -p patches-integration-tests` passes
- [ ] `cargo clippy -p patches-integration-tests` clean

## Notes

These tests follow the pattern established by
`patches-integration-tests/tests/file_params.rs` — build a real engine
from a `.patches` source string, tick it for a known number of samples,
and assert on output buffer contents.

Test fixture `.patches` files (if needed) go in
`patches-dsl/tests/fixtures/`.

Epic: E060
ADR: 0029
