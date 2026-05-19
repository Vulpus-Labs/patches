---
id: "0932"
title: Reorg — utils/ group dir
priority: medium
created: 2026-05-18
epic: E151
---

## Summary

Create `patches-modules/src/utils/` and move the utility modules
into it: `sum`, `poly_sum`, `vca`, `poly_vca`, `tap`, `mono_to_poly`,
`poly_to_mono`, `quant_util`.

## Acceptance criteria

- [ ] `patches-modules/src/utils/{mod.rs, sum.rs, poly_sum.rs,
      vca.rs, poly_vca.rs, tap.rs, mono_to_poly.rs, poly_to_mono.rs,
      quant_util.rs}` exist; flat siblings deleted.
- [ ] Public re-exports preserve `patches_modules::Sum`, `::Vca`,
      `::Tap`, etc.
- [ ] Tests pass unchanged beyond import-path edits.
- [ ] `cargo clippy --all-targets -- -D warnings` green.
- [ ] `just commit -p patches-modules` green.

## Notes

Structural only.
