---
id: "0524"
title: Split patches-interpreter lib.rs by phase
priority: medium
created: 2026-04-17
epic: E090
---

## Summary

[patches-interpreter/src/lib.rs](../../patches-interpreter/src/lib.rs)
is 670 lines bundling the `build` / `build_with_base_dir` /
`build_from_bound` entry points, the `BoundItem` / `require_resolved`
binding helpers, and tracker/song conversion (`build_tracker_data`,
`convert_step`, `validate_sequencer_songs`, `convert_value`,
`value_kind_name`).

The tracker/song conversion block is a self-contained phase that
only runs when a FlatPatch carries a `sequencer_data` section and
has no reverse dependency from the core build path.

## Acceptance criteria

- [ ] Convert `lib.rs` to `lib.rs` + sibling submodules:
      `tracker.rs` (build_tracker_data, convert_step, validate_sequencer_songs,
      convert_value, value_kind_name) and `binding.rs` (BoundItem impls,
      require_resolved helper).
- [ ] Public entry points (`build`, `build_with_base_dir`,
      `build_from_bound`, `BuildResult`) stay exported from `lib.rs`.
- [ ] `lib.rs` under ~300 lines.
- [ ] `cargo build -p patches-interpreter`, `cargo test -p patches-interpreter`,
      `cargo clippy` clean.

## Notes

E090. No behaviour change. If `tracker.rs` collides with the existing
`patches-core/src/tracker.rs` on name in readers' heads, use
`song_data.rs` instead.
