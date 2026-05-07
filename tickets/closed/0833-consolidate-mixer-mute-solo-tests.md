---
id: "0833"
title: Consolidate mute/solo tests across mixer variants
priority: low
created: 2026-05-07
epic: E138
---

## Summary

Mute and solo interaction is tested separately in `mixer`, `poly_mixer`,
and `stereo_poly_mixer` (mixer/tests.rs:86-97, poly_mixer:278-291,
stereo_poly_mixer:362-373). Logic is shared; tests are near-duplicates.

Also duplicated:

- `mixer/tests.rs:46-52` (unity sums) overlaps `mixer_send_buses_accumulate`
- `mixer/tests.rs:55-63` level-CV clamp duplicated in poly_mixer:244

Consolidate via a shared test helper or a single mute/solo truth-table run
once per variant with the variant-specific harness builder injected.

## Acceptance criteria

- [ ] One mute/solo test source per variant, ideally driven by a shared
      truth-table helper
- [ ] Redundant unity-sum and CV-clamp tests in `mixer/tests.rs` removed
- [ ] `just inner -p patches-modules` green

## Notes

If extracting a shared helper means leaking variant-internal types, prefer
deletion of the duplicates over a complex helper.
