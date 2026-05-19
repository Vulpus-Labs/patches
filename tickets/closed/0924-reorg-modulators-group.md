---
id: "0924"
title: Reorg — modulators/ group dir
priority: medium
created: 2026-05-18
epic: E151
---

## Summary

Create `patches-modules/src/modulators/` and move the modulator
modules into it: `adsr`, `poly_adsr`, `lfo`, `poly_lfo`, `sah`,
`poly_sah`, `glide`, `op`, `poly_op`, `quant`, `poly_quant`,
`tuner`, `poly_tuner`, `ring_mod`.

Subfiles per module / variant, e.g.
`modulators/adsr.rs` + `modulators/poly_adsr.rs`. `modulators/mod.rs`
declares submodules and `pub use` re-exports.

## Acceptance criteria

- [ ] `patches-modules/src/modulators/` exists with every listed
      subfile; flat siblings deleted.
- [ ] Public re-exports preserve every existing
      `patches_modules::Adsr`, `::PolyLfo`, `::RingMod`, etc.
- [ ] Tests pass unchanged beyond import-path edits.
- [ ] `cargo clippy --all-targets -- -D warnings` green.
- [ ] `just commit -p patches-modules` green.

## Notes

Structural only. Mono / poly variants stay as sibling files;
deeper unification (one module parameterised over axis) is out of
scope for this epic.
