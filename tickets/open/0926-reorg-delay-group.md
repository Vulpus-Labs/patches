---
id: "0926"
title: Reorg — delay/ group dir
priority: medium
created: 2026-05-18
epic: E151
---

## Summary

Create `patches-modules/src/delay/` and move `delay`,
`stereo_delay` into it.

## Acceptance criteria

- [ ] `patches-modules/src/delay/{mod.rs, delay.rs, stereo_delay.rs}`
      exist; flat siblings deleted.
- [ ] Public re-exports preserve `patches_modules::Delay`,
      `::StereoDelay`.
- [ ] Tests pass unchanged beyond import-path edits.
- [ ] `cargo clippy --all-targets -- -D warnings` green.
- [ ] `just commit -p patches-modules` green.

## Notes

Structural only.
