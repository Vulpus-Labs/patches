---
id: "0505"
title: Split patches-modules mixer by variant
priority: medium
created: 2026-04-16
---

## Summary

`patches-modules/src/mixer/mod.rs` is 662 lines and defines four
independent variants: `Mixer`, `StereoMixer`, `PolyMixer`,
`StereoPolyMixer`. Each has its own `Module` impl and parameter
handling.

## Acceptance criteria

- [ ] Add sibling submodules to the existing `mixer/` directory:
      `mono.rs` (Mixer), `stereo.rs` (StereoMixer),
      `poly.rs` (PolyMixer), `stereo_poly.rs` (StereoPolyMixer).
- [ ] `mixer/mod.rs` declares the submodules and re-exports the
      four types at their existing paths.
- [ ] Module registrations in `patches-modules` unchanged.
- [ ] `cargo build -p patches-modules`, `cargo test -p patches-modules`,
      `cargo clippy` clean.

## Notes

E086. Pattern mentioned in E085 summary: "mixer by variant type".
