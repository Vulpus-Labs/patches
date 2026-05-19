---
id: "0922"
title: Reorg — stereo/ group dir
priority: medium
created: 2026-05-18
epic: E151
---

## Summary

Create `patches-modules/src/stereo/` and move `stereo_split`,
`stereo_sum` into it. The new modules from ticket 0919 (`Pan`,
`Balance`, `StereoWidth`, `MidSide`, `MonoBass`) live here too; if
0919 has already landed in the flat layout, this ticket moves them.

Subfile pattern: one file per module (`stereo/pan.rs`,
`stereo/balance.rs`, etc.). `stereo/mod.rs` holds the doc block
and `pub use` re-exports.

## Acceptance criteria

- [ ] `patches-modules/src/stereo/` exists with one subfile per
      module; flat siblings deleted.
- [ ] Public re-exports preserve `patches_modules::StereoSplit`,
      `::StereoSum`, `::Pan`, etc.
- [ ] Tests pass unchanged beyond import-path edits.
- [ ] `cargo clippy --all-targets -- -D warnings` green.
- [ ] `just commit -p patches-modules` green.

## Notes

Structural only.
