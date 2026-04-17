---
id: "E087"
title: Test-file category splits for tests/*.rs
created: 2026-04-17
closed: 2026-04-17
status: closed
tickets: ["0520", "0521", "0522", "0523"]
---

## Summary

Tier C follow-on to E085. With inline `mod tests` extractions done in
E085 and structural impl splits done in E086, the remaining files over
~600 lines on the test side are four integration `tests/*.rs` files
that accumulate many categories into one monolithic module. Same
shape of split as [patches-dsl/tests/expand_tests.rs](../../patches-dsl/tests/expand_tests.rs):
the top-level `tests/foo.rs` becomes a thin stub that declares a
sibling `foo/` directory, and categories live as submodules under
`foo/mod.rs`.

Each `tests/*.rs` remains one integration test binary (cargo treats
each top-level file in `tests/` as a separate target); the stub keeps
that binary while the category files hang off it as submodules.

Mechanical. No behaviour change. Each ticket is independent and leaves
the crate's public surface untouched.

## Acceptance criteria

- [x] All 4 tickets (0520–0523) closed.
- [x] Every listed test file split along the category axes named in
      its ticket, with `tests/foo.rs` reduced to a stub that declares
      the `foo/` submodule tree.
- [x] `cargo build`, `cargo test`, `cargo clippy` clean at each ticket
      boundary and across the workspace at epic close.
- [x] No public API changes; no change in the set of integration test
      binaries produced.
- [x] Histogram rebaselined — no `tests/*.rs` file sits over ~600
      lines purely due to a missed category split addressed here.

## Notes on closure

Two stubs (`slot_deck.rs`, `tracker.rs`) required `#[path =
"<name>/mod.rs"] mod cases;` to disambiguate: when the stub file
and the submodule directory share a basename, `mod <name>;` is
ambiguous (Rust finds both `tests/<name>.rs` and
`tests/<name>/mod.rs`). `expand_tests.rs` / `torture_tests.rs` /
`parser_tests.rs` avoid the issue because their stub names differ
from their subdir names.

`slot_deck.rs` picked up a `pitch_shift.rs` category in addition
to the six named in ticket 0522 — the file had accreted a
spectral-pitch-shifter section after the ticket was drafted. Kept
under the same `slot_deck/` tree for the same category-split
rationale.

## Notes

Pattern reference: [patches-dsl/tests/expand_tests.rs](../../patches-dsl/tests/expand_tests.rs)
is a two-line stub that declares `mod support; mod expand;`, with
categories in `tests/expand/{alias_scope, arity, flat, param_interp,
patterns_songs, templates, warnings}.rs`. Follow the same convention:

```
tests/foo.rs            # stub: `mod foo;` (and `mod support;` if shared)
tests/foo/mod.rs        # declares category submodules
tests/foo/<category>.rs # one file per axis
tests/foo/support.rs    # shared helpers, if any
```

Out of scope:

- Changes to test logic, coverage, or fixture layout.
- Renaming public APIs under test.
- Splitting files under 600 lines (revisit after rebaseline).
