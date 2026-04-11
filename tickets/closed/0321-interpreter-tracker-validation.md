---
id: "0321"
title: "Interpreter: tracker validation rules"
priority: high
created: 2026-04-11
---

## Summary

Add interpreter validation for tracker-related semantic rules, producing
clear error messages for invalid pattern/song configurations.

## Acceptance criteria

- [ ] Error if a song references a pattern name that doesn't exist
- [ ] Error if a MasterSequencer's `song` parameter references an
      undefined song name
- [ ] Error if patterns within a single song column have different step
      counts
- [ ] Error if patterns within a single song column have different
      channel counts
- [ ] Error if a song's column headers don't match the declared channels
      of the MasterSequencer that references it
- [ ] Error messages include the relevant names and source spans
- [ ] Unit tests for each validation rule (both passing and failing
      cases)
- [ ] `cargo test -p patches-interpreter` passes
- [ ] `cargo clippy -p patches-interpreter` clean

## Notes

These validations are listed in ADR 0029 under "Interpreter validation".
Runtime channel count mismatch (pattern has more/fewer channels than the
player) is handled gracefully at runtime, not as an interpreter error —
but the interpreter validates consistency within a song column so this
shouldn't occur in normal use.

Epic: E059
ADR: 0029
