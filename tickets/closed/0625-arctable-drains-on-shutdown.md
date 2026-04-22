---
id: "0625"
title: ArcTable drains to zero on engine shutdown
priority: high
created: 2026-04-21
---

## Summary

End-to-end: load gain, run a patch that mints several buffer
ids (via `File` parameters resolved to `FloatBuffer`), shut
down the engine. Assert the `ArcTable` is empty and every
`Arc<[f32]>` dropped on the control thread / cleanup worker —
never audio thread.

## Acceptance criteria

- [ ] Integration test in `patches-integration-tests`.
- [ ] ArcTable exposes a test-only `len()` / `is_empty()` or
      equivalent assertion hook.
- [ ] Drop-thread check: a custom `Drop` on the Arc payload
      records `thread::current().id()`; test asserts it is not
      the audio thread id.

## Notes

Epic E107.
