---
id: "0191"
title: "`PolySah` module (polyphonic sample-and-hold)"
priority: medium
created: 2026-03-25
epic: "E035"
---

## Summary

Implement `PolySah` in `patches-modules/src/poly_sah.rs`. Identical behaviour
to `Sah` (T-0190) but operates on 16 voices. A single mono `trig` broadcasts to
all voices so they latch simultaneously.

## Acceptance criteria

- [ ] `patches-modules/src/poly_sah.rs` defines `pub struct PolySah`.
- [ ] `PolySah::describe` returns a descriptor with:
  - `poly_in("in")`
  - `mono_in("trig")`
  - `poly_out("out")`
  - No parameters
- [ ] `PolySah::prepare` initialises `held: [f32; 16] = [0.0; 16]`,
  `prev_trig: f32 = 0.0`.
- [ ] `set_ports` assigns `in_sig: PolyInput`, `in_trig: MonoInput`,
  `out: PolyOutput`.
- [ ] `process` logic:
  - Read mono `trig = pool.read_mono(&self.in_trig)`.
  - On rising edge (`self.prev_trig < 0.5 && trig >= 0.5`):
    - Read `voices = pool.read_poly(&self.in_sig)`.
    - Copy `self.held = voices` (element-wise; `[f32; 16]` is `Copy`).
  - `self.prev_trig = trig`.
  - Write `pool.write_poly(&self.out, self.held)`.
- [ ] No `unwrap()` / `expect()`.
- [ ] `cargo clippy` clean, all existing tests pass.

## Notes

Depends on T-0190 (same file layout convention). Export from `lib.rs` but don't
register yet (T-0194).
