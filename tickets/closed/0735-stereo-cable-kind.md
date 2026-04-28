---
id: "0735"
title: Add CableKind::Stereo and StereoInput/StereoOutput ports
priority: high
created: 2026-04-27
---

## Summary

Introduce `CableKind::Stereo` and the matching `StereoInput` /
`StereoOutput` port types in `patches-core`. Storage reuses
`CableValue::Poly([f32; 16])` with only lanes 0–1 occupied (`L`, `R`).
Type checker rejects all existing kind mismatches; coercion (0736) is a
follow-up.

## Acceptance criteria

- [ ] `CableKind::Stereo` variant added; all existing match arms in
      `patches-core`, planner, and graph validator updated.
- [ ] `StereoInput`/`StereoOutput` structs added in
      `patches-core/src/cables/`.
- [ ] `DescriptorBuilder` gains `.stereo_in(name)` and
      `.stereo_out(name)`.
- [ ] Stereo→mono and stereo↔poly connections fail with
      `CableKindMismatch`.
- [ ] Stereo cables allocate as poly slots; cable pool unchanged.
- [ ] `cargo test -p patches-core` green.

## Notes

ADR 0059 §1, §3. No mono→stereo broadcast yet — that lands in 0736.
Keep the stereo lane access (`read()`/`write()`) returning `(f32, f32)`
or a small `StereoSample` newtype; do not expose the underlying
`[f32; 16]` to module code.
