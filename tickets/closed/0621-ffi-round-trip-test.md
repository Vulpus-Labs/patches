---
id: "0621"
title: FFI round-trip test — encode → extern C → decode parity
priority: high
created: 2026-04-21
---

## Summary

End-to-end: host builds a `ParamFrame` from a populated
`ParameterMap`, loads the gain dylib, calls
`update_validated_parameters` through the real `extern "C"`
boundary, reads back the plugin's internal state (via a debug
accessor or a sentinel output) and asserts every value matches.

## Acceptance criteria

- [ ] Test in `patches-integration-tests` covers one frame with
      every `ScalarTag` represented plus a buffer id.
- [ ] Uses the real gain dylib (+ a test-only debug accessor
      export if needed for state inspection).
- [ ] Green under both default and
      `audio-thread-allocator-trap` features.

## Notes

Epic E107.
