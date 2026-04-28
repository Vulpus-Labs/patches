---
id: "0736"
title: Migrate pitch_shift, delay, stereo_delay to structural params
priority: high
created: 2026-04-28
epic: "E126"
adrs: ["0060"]
depends_on: ["0734", "0735"]
---

## Summary

Reintroduce `length` and `high_quality` as structural params on the
modules that consume them. Pure mechanical migration: declare the
params in `describe`, read them from `&StructuralParams` in `prepare`
instead of `descriptor.shape.*`, drop the temporary hard-coded
defaults from 0735.

## Acceptance criteria

- [ ] `pitch_shift`: `length` declared as a structural `int` param
      (range matching the previous `shape.length` validation,
      default `0`). `high_quality` declared as a structural `bool`
      (default `false`). Both read in `prepare`.
- [ ] `delay`: `high_quality` declared as a structural `bool` (default
      `false`). Read in `prepare`.
- [ ] `stereo_delay`: `high_quality` declared as a structural `bool`
      (default `false`). Read in `prepare`.
- [ ] DSL: previously `Foo(channels: N)` with no way to spell
      `high_quality`; now `Foo(N) { high_quality: true }`. Write a
      test patch exercising the param to confirm wiring end-to-end.
- [ ] Existing tests for these modules updated to pass structural
      params via `ModuleHarness::build_full` rather than the old
      `ModuleShape { high_quality: true }` route. Test coverage of
      both `high_quality: false` and `high_quality: true` paths
      preserved.
- [ ] `cargo test -p patches-modules -p patches-engine` passes.

## Notes

The `length` param on `pitch_shift` controls FFT processing budget —
preserve the existing clamping logic (`next_power_of_two().clamp(128, 4096)`
when non-zero) inside `prepare` rather than at descriptor declaration.
