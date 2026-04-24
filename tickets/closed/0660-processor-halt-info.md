---
id: "0660"
title: Processor::halt_info() control-thread query
priority: high
created: 2026-04-24
epic: E113
adr: 0051
depends_on: ["0659"]
---

## Summary

Expose halt state from `Processor` to the control thread so hosts can
display a diagnostic. Non-blocking, lock-free read path.

## Acceptance criteria

- [ ] `Processor::halt_info(&self) -> Option<HaltInfoSnapshot>` where
      `HaltInfoSnapshot` is a small owned struct (slot index, module
      name `String`, payload `String`). Returns `None` when not halted.
- [ ] Implementation reads `halted` with `Acquire`; if true, copies the
      `HaltInfo` cell under its mutex (control-thread only, so mutex is
      acceptable).
- [ ] Integration test covers the full path: panicking module, audio
      thread halts, control thread observes `Some(...)` on next poll.
- [ ] On plan adoption (rebuild), `halted` is cleared and `halt_info`
      becomes `None` again. Covered by existing plan-adoption tests
      extended with a pre-halt set-up.

## Notes

Do not expose `HaltInfo` directly — callers should get an owned snapshot
so the mutex is released before any UI work.
