---
id: "0339"
title: Refactor Sah and PolySah to use TriggerInput
priority: medium
created: 2026-04-12
---

## Summary

Replace `prev_trig: f32` + `in_trig: MonoInput` in `Sah` and `PolySah`
with `TriggerInput`. Both modules use a mono trigger (PolySah shares a
single trigger across all poly voices).

## Acceptance criteria

- [ ] `sah.rs`: `prev_trig` removed, `in_trig` changed to `TriggerInput`
- [ ] `poly_sah.rs`: same (mono `TriggerInput`, not poly)
- [ ] `cargo test -p patches-modules` passes
- [ ] `cargo clippy -p patches-modules` clean

## Notes

Depends on 0335. See ADR 0030. Epic E062.
