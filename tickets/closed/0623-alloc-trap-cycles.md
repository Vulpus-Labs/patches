---
id: "0623"
title: 10 000 process cycles under allocator trap (FFI path)
priority: high
created: 2026-04-21
---

## Summary

Run the gain dylib for 10 000 `process` cycles (plus
interleaved `update_validated_parameters` and `set_ports`
calls) under `patches-alloc-trap/audio-thread-allocator-trap`.
Assert no trap fires.

## Acceptance criteria

- [ ] Integration test in `patches-integration-tests` gated on
      the `audio-thread-allocator-trap` feature.
- [ ] Includes both pure-process cycles and parameter-update
      cycles.
- [ ] Runs in CI.

## Notes

Epic E107. Mirrors the Spike 4 in-process sweep.
