---
id: "0584"
title: Multi-threaded soak + Miri + exhaustion coverage for ArcTable
priority: high
created: 2026-04-19
---

## Summary

Bring the `ArcTable` stack up to the "secure before moving on"
bar for ADR 0045 spike 2. Unit tests in 0581–0583 cover the
single-threaded happy paths; this ticket adds the adversarial
coverage the ADR spike list calls out: multi-threaded soak,
exhaustion, and a Miri pass on the atomic-heavy module.

## Acceptance criteria

- [ ] Soak test (ignored-by-default `#[test]`, opt-in via
      `--ignored`): one control thread continuously minting new
      ids and sometimes releasing them directly; one "audio"
      thread retaining and releasing a random subset with
      short sleeps. Runs for a fixed number of iterations
      (default 100 000, overridable via env var). At the end:
      drain completes; every minted `Arc` reaches
      `strong_count == 1` (held only by the test harness),
      confirming no leaks and no premature drops.
- [ ] Exhaustion test: tight capacity (e.g. 8), mint until
      `ArcTableError::Exhausted`, release some, verify further
      mints succeed. Confirms slot reuse works after drain.
- [ ] Stale-id test (debug-only): fabricate an id with a wrong
      generation for an occupied slot, confirm the debug assert
      in `retain` fires. In release builds, confirm the test is
      skipped rather than silently passing.
- [ ] Miri job: `cargo +nightly miri test -p patches-ffi-common
      arc_table` runs clean. Document the invocation in
      `CONTRIBUTING` or the crate README, and add a CI entry if
      the workspace has one for Miri already; otherwise leave a
      note on the ticket rather than block on CI plumbing.
- [ ] `cargo test -p patches-ffi-common` (excluding ignored
      tests) clean; clippy clean.

## Notes

Determinism: the soak test uses a seeded RNG so failures are
reproducible; surface the seed in the assertion message.

The stale-id test lives under `#[cfg(debug_assertions)]`; the
release-build path confirms `retain` still produces the
expected refcount behaviour (because the decoder has already
validated the id at frame-dispatch) — document this gap
explicitly so readers do not mistake the debug assert for a
runtime-enforced invariant.
