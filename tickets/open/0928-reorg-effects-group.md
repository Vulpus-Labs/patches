---
id: "0928"
title: Reorg — effects/ group dir
priority: medium
created: 2026-05-18
epic: E151
---

## Summary

Create `patches-modules/src/effects/` and move `bitcrusher`,
`drive` into it.

## Acceptance criteria

- [ ] `patches-modules/src/effects/{mod.rs, bitcrusher.rs, drive.rs}`
      exist; flat siblings deleted.
- [ ] Public re-exports preserve `patches_modules::Bitcrusher`,
      `::Drive`.
- [ ] Tests pass unchanged beyond import-path edits.
- [ ] `cargo clippy --all-targets -- -D warnings` green.
- [ ] `just commit -p patches-modules` green.

## Notes

Structural only.
