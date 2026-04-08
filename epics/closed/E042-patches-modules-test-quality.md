# E042 — patches-modules test quality

## Goal

After E041, the DSP kernels extracted to `patches-dsp` have their own rigorous,
independent tests. The `patches-modules` test suite should therefore be pruned of
anything that merely re-tests kernel behaviour, filled out to give the poly modules
the same coverage as their mono counterparts, and tidied to follow a single
consistent idiom throughout.

After this epic:

- No test in `patches-modules` duplicates a property already guaranteed by a
  `patches-dsp` test.
- `PolyOsc` and `PolyAdsr` test suites are at feature-parity with `Oscillator` and
  `Adsr` respectively.
- All module test files follow the same harness idiom: a `make_*` factory helper,
  a shared `env()` or equivalent, and documented `assert_within!` tolerances.

## Background

Review conducted in March 2026 identified three categories of issue:

1. **Redundant tests** — `Adsr::output_clamped_to_unit_range` and the
   oscillator cycle-consistency tests cover properties that are now directly
   guaranteed by `patches-dsp` tests (T4 stability / T7 determinism / T2
   frequency response). Keeping them adds noise and creates a maintenance burden
   without adding safety.

2. **Coverage gaps** — `PolyOsc` has 2 tests (vs. 9 for `Oscillator`); `PolyAdsr`
   has 4 (vs. 5 for `Adsr`). Key cases — sentinel disconnection, per-voice
   waveform shape, sustain/release/retrigger per voice — are completely absent.
   `PolyNoise` lacks a poly equivalent of the smoothness-hierarchy test.

3. **Idiom inconsistency** — `poly_osc.rs` lacks a `make_*` helper;
   `AudioEnvironment` is constructed inline in some files and via a helper in
   others; `assert_within!` tolerances carry no explanation; one `poly_adsr` test
   verifies harness behaviour rather than module behaviour.

## Tickets

| #      | Title                                                              | Priority |
|--------|--------------------------------------------------------------------|----------|
| T-0219 | Remove redundant patches-modules tests superseded by patches-dsp   | low      |
| T-0220 | Expand PolyOsc tests to match mono Oscillator coverage             | medium   |
| T-0221 | Expand PolyAdsr tests to match mono Adsr coverage                  | medium   |
| T-0222 | Standardise patches-modules test harness idiom                     | low      |
