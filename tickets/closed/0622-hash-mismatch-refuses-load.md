---
id: "0622"
title: Descriptor hash mismatch refuses load
priority: high
created: 2026-04-21
---

## Summary

Build a mutated copy of the gain plugin whose exported
`descriptor_hash()` returns a wrong value (or whose descriptor
differs from the host's expected descriptor). Attempt to load.
Assert the loader returns an error and the plugin's `create` is
never invoked.

## Acceptance criteria

- [ ] Fixture plugin under `patches-ffi/tests/fixtures/` or a
      feature-gated build variant of gain.
- [ ] Loader returns a descriptive error mentioning both hashes.
- [ ] A tracing span / counter confirms `create` was not called.

## Notes

Epic E107.
