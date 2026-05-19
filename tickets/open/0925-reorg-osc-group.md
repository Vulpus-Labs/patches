---
id: "0925"
title: Reorg — osc/ group dir
priority: medium
created: 2026-05-18
epic: E151
---

## Summary

Create `patches-modules/src/osc/` and move `oscillator`,
`poly_osc`, `noise` into it.

## Acceptance criteria

- [ ] `patches-modules/src/osc/{mod.rs, oscillator.rs, poly_osc.rs,
      noise.rs}` exist; flat siblings deleted.
- [ ] Public re-exports preserve `patches_modules::Oscillator`,
      `::PolyOsc`, `::Noise`.
- [ ] Tests pass unchanged beyond import-path edits.
- [ ] `cargo clippy --all-targets -- -D warnings` green.
- [ ] `just commit -p patches-modules` green.

## Notes

Structural only.
