---
id: "0784"
title: Investigate tracker pattern_switching / loop_swing test regressions
priority: medium
created: 2026-05-02
---

## Summary

Two integration tests in `patches-integration-tests/tests/tracker/` fail on
clean main and have done so independently of recent LSP / channels-validation
work:

- `cases::pattern_switching::pattern_switching_at_row_boundary`
- `cases::loop_swing::loop_row_is_not_skipped`

Both build a small patch with `MasterSequencer → PatternPlayer` and read
`out.last_right()` expecting a single-sample-high pulse on the first `x`
step. Actual output is 0 across the 6-sample scan window, so the assertion
`high.len() == 1` fails.

## Acceptance criteria

- [ ] Identify the commit that broke the test (likely candidates:
      [3b830a3](ADR 0047 sub-sample triggers — VDco/Osc/PolyOsc/LFO
      reset_out + sync_in) or [30ef5ed](0768 input-port offset + clip
      runtime), based on history of touched modules).
- [ ] Determine whether the bug is in PatternPlayer's `cv1` emission,
      MasterSequencer's clock bus, or the test's expectation of how
      `cv1` for an `x` step renders (continuous-hold vs. one-sample
      pulse).
- [ ] Fix and re-enable both tests, or rewrite them against the actual
      intended semantics with a comment explaining what changed.

## Notes

The patches both wire `player.cv1[kick] -> out.in` (mono → stereo
broadcast) and dangle `player.trigger[kick] -> t2a.in` (SyncToTrigger
output is unconnected). `last_right()` therefore reads `cv1`, not the
trigger — possible the test was written when `cv1` for `x` emitted a
1-sample pulse aligned with the trigger, and a later refactor switched
it to sample-and-hold semantics (in which case the assertion should
read ~`tick_samples` highs, not 1).

Failures reproduce with my channels-validation work stashed and with
the user's WIP bind-validation changes (tickets 0782/0783) stashed —
unrelated to either.
