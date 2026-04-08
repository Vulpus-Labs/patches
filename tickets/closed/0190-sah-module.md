---
id: "0190"
title: "`Sah` module (mono sample-and-hold)"
priority: medium
created: 2026-03-25
epic: "E035"
---

## Summary

Implement `Sah` (sample-and-hold) in `patches-modules/src/sah.rs`. On each
rising edge of `trig` (signal crossing 0.5 from below), the current value of
`in` is latched and held on `out` until the next trigger.

## Acceptance criteria

- [ ] `patches-modules/src/sah.rs` defines `pub struct Sah`.
- [ ] `Sah::describe` returns a `ModuleDescriptor` with:
  - `mono_in("in")` — signal to sample
  - `mono_in("trig")` — trigger input
  - `mono_out("out")` — held output
  - No parameters
- [ ] `Sah::prepare` initialises `held = 0.0`, `prev_trig = 0.0`.
- [ ] `set_ports` assigns `in_sig`, `in_trig`, `out`.
- [ ] `process` logic:
  - Read `trig = pool.read_mono(&self.in_trig)`.
  - If `self.prev_trig < 0.5 && trig >= 0.5` (rising edge), read
    `self.held = pool.read_mono(&self.in_sig)`.
  - `self.prev_trig = trig`.
  - Write `pool.write_mono(&self.out, self.held)`.
- [ ] No `unwrap()` / `expect()`.
- [ ] `cargo clippy` clean, all existing tests pass.

## Notes

No parameters are needed — trigger threshold is fixed at 0.5. This matches the
convention used for trigger detection elsewhere in the project (if such a
convention exists; check before writing).

The module should be exported from `patches-modules/src/lib.rs` but **not** yet
registered in `default_registry()`; that happens in T-0194.
