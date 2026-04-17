---
id: "0520"
title: Split patches-dsl torture_tests.rs by category
priority: low
created: 2026-04-17
epic: E087
---

## Summary

[patches-dsl/tests/torture_tests.rs](../../patches-dsl/tests/torture_tests.rs)
is 1039 lines organised into six numbered sections. Convert it to a
stub that declares a `torture/` submodule tree, one file per section.

## Acceptance criteria

- [ ] `patches-dsl/tests/torture_tests.rs` reduced to a stub
      (`mod support;` + `mod torture;`).
- [ ] `patches-dsl/tests/torture/mod.rs` declares the category
      submodules listed below.
- [ ] Each category submodule contains the tests from its matching
      numbered section, verbatim; no test logic edits.
- [ ] `cargo test -p patches-dsl --test torture_tests` passes with
      the same test count as before.
- [ ] `cargo build`, `cargo test`, `cargo clippy` clean workspace-wide.

## Target layout

```
patches-dsl/tests/torture_tests.rs            # stub
patches-dsl/tests/torture/mod.rs              # submodule declarations
patches-dsl/tests/torture/deep_alias.rs       # section 1
patches-dsl/tests/torture/arity.rs            # section 2 (arity / index forms)
patches-dsl/tests/torture/circular.rs         # section 3 (mutual / three-way cycles)
patches-dsl/tests/torture/mistyped.rs         # section 4 (mistyped call-site values)
patches-dsl/tests/torture/edge_cases.rs       # section 5
patches-dsl/tests/torture/scale.rs            # section 6 (scale composition)
```

`mod support;` stays at the stub level so `use support::*;` keeps
working from each category file via `use super::super::support::*;`
(or lift the shared helpers into `torture/support.rs` if cleaner).

## Notes

Pattern: [patches-dsl/tests/expand_tests.rs](../../patches-dsl/tests/expand_tests.rs)
+ [patches-dsl/tests/expand/](../../patches-dsl/tests/expand/). Part of
epic E087 (tier C follow-on to E085).
