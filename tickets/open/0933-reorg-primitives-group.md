---
id: "0933"
title: Reorg — primitives/ group dir
priority: medium
created: 2026-05-18
epic: E151
---

## Summary

Create `patches-modules/src/primitives/` to host the new
`DcBlocker` and `Comb` modules from ticket 0920. If 0920 lands
first into the flat layout, this ticket moves the files; if this
ticket lands first, 0920 writes directly into the new directory.

Coordinate with the ticket that lands first; do not duplicate work.

## Acceptance criteria

- [ ] `patches-modules/src/primitives/{mod.rs, dc_blocker.rs,
      comb.rs}` exist.
- [ ] Public re-exports preserve `patches_modules::DcBlocker`,
      `::Comb`.
- [ ] Tests pass unchanged beyond import-path edits.
- [ ] `cargo clippy --all-targets -- -D warnings` green.
- [ ] `just commit -p patches-modules` green.

## Notes

Structural only.
