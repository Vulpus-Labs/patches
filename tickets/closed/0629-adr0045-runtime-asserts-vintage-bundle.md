---
id: "0629"
title: ADR 0045 runtime asserts for vintage bundle (bit-identical + alloc trap)
priority: high
created: 2026-04-22
epic: "E109"
---

## Summary

Phase E of Spike 8. After 0572 runs the vintage patch end-to-end
via the bundle, add the ADR 0045-specific runtime assertions that
prove the new data plane behaves identically to the in-process
path.

## Acceptance criteria

- [ ] Retarget the parity test from 0628 to load `patches-vintage`
      via `PluginScanner` and render the same fixed-input patch.
      Assert the output WAV hash matches the committed baseline
      byte-for-byte.
- [ ] Enable the `audio-thread-allocator-trap` feature for an
      integration test that runs the full render cycle. No trap
      fires.
- [ ] Negative test: tamper with the built dylib's descriptor
      hash (e.g. a test helper that rewrites the exported hash
      symbol) and assert `PluginScanner` refuses to load.
- [ ] Refcount audit in debug: on teardown the ArcTable reports
      zero live ids attributable to the bundle.

## Notes

The tamper test can also be expressed by a second test bundle
whose descriptor computes a deliberately-wrong hash; pick
whichever is cheaper.

This ticket lands alongside 0572 — they share fixtures and
harness. Keep them as separate tickets so 0572 can close on its
original E095 acceptance criteria without pulling the ADR 0045
assertions inside its scope.
