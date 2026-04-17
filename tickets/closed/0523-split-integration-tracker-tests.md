---
id: "0523"
title: Split patches-integration-tests tracker.rs by category
priority: low
created: 2026-04-17
epic: E087
---

## Summary

[patches-integration-tests/tests/tracker.rs](../../patches-integration-tests/tests/tracker.rs)
is 680 lines exercising the tracker sequencer pipeline (DSL → interpreter
→ plan → audio-thread execution). Shared helpers (`env`, `registry`,
`load_fixture`, `build_engine`) live at the top of the file. Split
into a `tracker/` submodule tree with a dedicated `support.rs`.

## Acceptance criteria

- [ ] `patches-integration-tests/tests/tracker.rs` reduced to a stub
      (`mod tracker;`).
- [ ] `patches-integration-tests/tests/tracker/mod.rs` declares the
      category submodules listed below.
- [ ] Shared helpers (`env`, `registry`, `load_fixture`,
      `build_engine`) lifted to `tracker/support.rs`.
- [ ] Each category submodule contains the tests from its matching
      section, verbatim; no test logic edits.
- [ ] `cargo test -p patches-integration-tests --test tracker` passes
      with the same test count as before.
- [ ] `cargo build`, `cargo test`, `cargo clippy` clean workspace-wide.

## Target layout

```
patches-integration-tests/tests/tracker.rs                    # stub
patches-integration-tests/tests/tracker/mod.rs                # submodule declarations
patches-integration-tests/tests/tracker/support.rs            # env, registry, load_fixture, build_engine
patches-integration-tests/tests/tracker/transport.rs          # tracker_basic_round_trip, song_basic_builds_and_ticks, transport_no_autostart_silent
patches-integration-tests/tests/tracker/pattern_switching.rs  # pattern_switching_at_row_boundary
patches-integration-tests/tests/tracker/loop_swing.rs         # song_loop_point, loop_row_is_not_skipped, swing_alternates_tick_durations
patches-integration-tests/tests/tracker/slides_repeats.rs     # pattern_with_slides, pattern_with_repeats, repeat_retrigger_audible_*
```

## Notes

Pattern: [patches-dsl/tests/expand_tests.rs](../../patches-dsl/tests/expand_tests.rs)
+ [patches-dsl/tests/expand/](../../patches-dsl/tests/expand/). Part of
epic E087 (tier C follow-on to E085).
