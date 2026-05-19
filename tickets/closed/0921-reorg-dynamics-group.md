---
id: "0921"
title: Reorg — dynamics/ group dir
priority: medium
created: 2026-05-18
epic: E151
---

## Summary

Create `patches-modules/src/dynamics/` and move `limiter`,
`stereo_limiter`, `transient_shaper` into it. Subfile pattern
matches `mixer/`: one file per variant
(`dynamics/limiter.rs`, `dynamics/stereo_limiter.rs`,
`dynamics/transient_shaper.rs`, plus the comp/gate landings from
tickets 0915 / 0916 if those have landed). `dynamics/mod.rs` holds
the doc block and `pub use` re-exports.

If kernel work from 0915/0916 has already produced a
`dynamics/common/` submodule for the comp detector, it stays where
it is.

## Acceptance criteria

- [ ] `patches-modules/src/dynamics/{mod.rs, limiter.rs,
      stereo_limiter.rs, transient_shaper.rs}` exist; flat
      siblings deleted.
- [ ] `mod.rs` declares the submodules and re-exports the public
      types.
- [ ] Top-level `patches_modules::Limiter`, `::StereoLimiter`,
      `::TransientShaper` still resolve (and `::Compressor` /
      `::Gate` etc. if those have landed).
- [ ] No behavioural diff: tests for the moved modules pass
      unchanged (only import-path edits in test files allowed).
- [ ] `cargo clippy --all-targets -- -D warnings` green.
- [ ] `just commit -p patches-modules` green.

## Notes

Structural only. No drive-by cleanup, no extracting helpers, no
renaming ports. If something itches to be fixed, open a follow-up
ticket instead.
