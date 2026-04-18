---
id: "0536"
title: Split patches-interpreter src/tests.rs by category
priority: medium
created: 2026-04-17
epic: E090
---

## Summary

[patches-interpreter/src/tests.rs](../../patches-interpreter/src/tests.rs)
is 717 lines covering: happy-path build tests
(`build_single_module_patch`, `build_two_modules_with_connection`,
`forward_references_are_not_errors`), error-surface tests
(`unknown_*`, `empty_flat`, etc.), and song/sequencer conversion
tests that live at the bottom of the file.

## Acceptance criteria

- [ ] Convert to stub `src/tests.rs` declaring a submodule tree under
      `src/tests/` — or, since `tests.rs` sits next to `lib.rs`,
      convert to `src/tests/mod.rs` + category files and add
      `#[cfg(test)] mod tests;` inside `lib.rs`.
- [ ] Category split (final naming the ticket's call):
      - `happy_path.rs` — successful build scenarios
      - `errors.rs` — unknown-type / unknown-port / empty-patch
        error surfaces
      - `song_sequencer.rs` — sequencer-song conversion tests that
        pair with ticket 0524's extracted `tracker.rs` module
- [ ] Fixture builders (span/env/registry/osc_module/sum_module/
      connection helpers) in `tests/mod.rs` or a `tests/support.rs`.
- [ ] `cargo test -p patches-interpreter` passes with the same test
      count.
- [ ] `cargo build -p patches-interpreter`, `cargo clippy` clean.

## Notes

E090. No test logic edits. Coordinate with 0524 if they land together
— `song_sequencer.rs` exercises the extracted tracker module.
