---
id: "0690"
title: cables/gate.rs + cables/trigger.rs — coverage gaps
priority: medium
created: 2026-04-24
epic: E117
---

## Summary

0681 flagged `cables/gate.rs` with 14/26 (54%) and `cables/trigger.rs`
with 6/20 (30%) survived mutants. Gate/trigger semantics are
behavioural and audio-adjacent — worth pinning.

## Acceptance criteria

- [ ] Review MISSED lists.
- [ ] Add tests covering gate rising/falling edge logic and trigger
      one-shot semantics that mutants revealed.
- [ ] Re-run and record residual MISSED.
