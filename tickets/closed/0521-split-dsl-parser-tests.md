---
id: "0521"
title: Split patches-dsl parser_tests.rs by category
priority: low
created: 2026-04-17
epic: E087
---

## Summary

[patches-dsl/tests/parser_tests.rs](../../patches-dsl/tests/parser_tests.rs)
is 881 lines covering parser positive/negative fixtures, literal
parsing (unit / dB / note / note-like), error location accuracy, span
tightness, and pattern/song block parsing. Split along those axes.

## Acceptance criteria

- [ ] `patches-dsl/tests/parser_tests.rs` reduced to a stub
      (`mod parser;`).
- [ ] `patches-dsl/tests/parser/mod.rs` declares the category
      submodules listed below.
- [ ] `assert_parse_error_contains` helper lifted to
      `parser/support.rs` (or kept in `parser/mod.rs`) so every
      category can share it.
- [ ] Each category submodule contains the tests from its matching
      section, verbatim; no test logic edits.
- [ ] `cargo test -p patches-dsl --test parser_tests` passes with
      the same test count as before.
- [ ] `cargo build`, `cargo test`, `cargo clippy` clean workspace-wide.

## Target layout

```
patches-dsl/tests/parser_tests.rs               # stub
patches-dsl/tests/parser/mod.rs                 # submodule declarations
patches-dsl/tests/parser/support.rs             # shared `assert_parse_error_contains` etc.
patches-dsl/tests/parser/positive.rs            # positive fixtures
patches-dsl/tests/parser/literals.rs            # unit / dB / note / note-like literals
patches-dsl/tests/parser/negative.rs            # negative fixtures + literal error propagation
patches-dsl/tests/parser/error_locations.rs     # T-0248 error location accuracy
patches-dsl/tests/parser/spans.rs               # connection / module_decl span tightness
patches-dsl/tests/parser/pattern_song.rs        # pattern + song block parsing
```

## Notes

Pattern: [patches-dsl/tests/expand_tests.rs](../../patches-dsl/tests/expand_tests.rs)
+ [patches-dsl/tests/expand/](../../patches-dsl/tests/expand/). Part of
epic E087 (tier C follow-on to E085).
