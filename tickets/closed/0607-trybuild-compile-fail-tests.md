---
id: "0607"
title: Compile-fail tests for typed `ParamView::get` misuse
priority: low
created: 2026-04-21
---

## Summary

ADR 0046 moves three classes of runtime parameter bug to compile
errors: wrong kind (reading a `Float` slot as `i64`), scalar-vs-array
mismatch (using a `*ParamArray` name without `.at(i)`), and undefined
names (referencing a `params::foo` const that does not exist). Prove
the guarantees with `trybuild`-style compile-fail tests so a
regression in the typed surface is caught at CI time rather than at
the next audit.

## Acceptance criteria

- [ ] `trybuild` added as a dev-dependency of `patches-core` (check
      workspace policy: ask before adding if not already present).
- [ ] `patches-core/tests/compile_fail/` houses the `.rs` fixtures and
      their expected `.stderr` files.
- [ ] At least three fixtures covering:
  - `wrong_kind.rs` — `let _: i64 = view.get(params::dry_wet);`
    where `dry_wet` is `Float`.
  - `array_without_index.rs` — `view.get(params::gain);` where
    `gain` is a `FloatParamArray` (must require `.at(i)`).
  - `undefined_name.rs` — `view.get(params::not_a_real_param);`.
- [ ] A harness test (`tests/compile_fail.rs`) invoking
      `trybuild::TestCases::new().compile_fail("tests/compile_fail/*.rs")`.
- [ ] `cargo test -p patches-core` runs the compile-fail suite and
      passes.

## Notes

`trybuild` pins stderr output; regenerate with
`TRYBUILD=overwrite cargo test -p patches-core compile_fail` when the
compiler's diagnostic phrasing changes. Keep fixtures minimal
(one-line `fn main()` consuming a shared helper) so stderr stays
stable across rustc releases.

If `trybuild` is disallowed as a dep, an acceptable fallback is a
single negative test using `compiletest-rs` or a hand-rolled build.rs
stanza, but `trybuild` is idiomatic for this shape of test and the
upstream cost is ~1 crate.

Closes the final unchecked box on ticket 0605 § Phase B step 6.
