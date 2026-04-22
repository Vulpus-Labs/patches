---
id: "0624"
title: Double-release audit trap in debug builds
priority: high
created: 2026-04-21
---

## Summary

A debug-build fixture plugin that calls `float_buffer_release`
twice for the same id. The `ArcTable` refcount audit must trip
(panic or explicit abort) — this is the test that proves the
audit works against a real ABI caller.

## Acceptance criteria

- [ ] Fixture plugin variant that intentionally double-releases.
- [ ] Test asserts the audit fires (catch_unwind or
      `should_panic`).
- [ ] Release builds: no audit, no panic, just a leaked
      refcount — documented behaviour.

## Notes

Epic E107. ArcTable audit machinery landed in Spike 2 / Spike 6.
