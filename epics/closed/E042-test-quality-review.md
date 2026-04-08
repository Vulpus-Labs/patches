# E042 — Test quality review

## Goal

Improve test quality, coverage and maintainability across the workspace by:
applying the new shared test vocabulary (helpers, macros, harnesses); filling
critical coverage gaps; consolidating verbose or low-value tests into concise,
intent-revealing forms.

## Background

A cross-crate test review identified five categories of improvement:

1. **Coverage gaps** — biquad/FFT/convolver have thin or missing frequency-
   response and round-trip tests; `cable_pool` lacks a test for its core
   ping-pong invariant.
2. **Integration test duplication** — every integration test file redefines
   `ImpulseSource`, `ConstSource`, `SineSource`, `p()`, `env()`, constants.
   These are now consolidated in `patches-integration-tests/src/lib.rs`.
3. **DSP test duplication** — determinism/reset tests and `rms`/`sine_rms`
   helpers are copy-pasted across 7+ files. New macros
   (`assert_deterministic!`, `assert_reset_deterministic!`) and functions
   (`rms`, `sine_rms_warmed`) are now in `patches-dsp/src/test_support.rs`.
4. **Cable pool invariant** — the 1-sample ping-pong delay is the execution
   model's defining property but has no explicit test.
5. **Low-value test consolidation** — tautological point-check tests,
   redundant noise-range checks, and descriptor-shape tests can be collapsed
   into parameterised forms.

## New shared vocabulary (already landed)

| Location | What was added |
|---|---|
| `patches-dsp/src/test_support.rs` | `rms()`, `sine_signal()`, `sine_rms_warmed()`, `assert_deterministic!`, `assert_reset_deterministic!` |
| `patches-core/src/test_support/harness.rs` | `ModuleHarness::measure_rms()`, `measure_peak()`, `assert_output_bounded()`, `disconnect_inputs()` |
| `patches-core/src/test_support/macros.rs` | `assert_attenuated!`, `assert_passes!` |
| `patches-integration-tests/src/lib.rs` | `POOL_CAP`, `MODULE_CAP`, `SAMPLE_RATE`, `p()`, `pi()`, `env()`, `env_at()`, `build_engine()`, `build_engine_with()`, `run_n_left()`, `run_n_stereo()`, `ImpulseSource`, `ConstSource`, `SineSource` |
| `patches-dsl/tests/support/mod.rs` | `parse_expand()`, `parse_expand_err()`, `module_ids()`, `find_module()`, `get_param()`, `connection_keys()`, `find_connection()`, `assert_modules_exist()`, `assert_connection_scale()` |

## Suggested ordering

1. T-0240 — Fill biquad/FFT/convolver coverage gaps (highest risk reduction).
2. T-0241 — Add cable_pool ping-pong invariant test.
3. T-0242 — Migrate integration tests to shared infrastructure.
4. T-0243 — Migrate DSP determinism tests to shared macros.
5. T-0244 — Consolidate low-value tests into parameterised forms.
6. T-0245 — Migrate DSL tests to shared support module.
7. T-0246 — Apply `ModuleHarness` helpers across `patches-modules` tests.

## Tickets

| # | Title | Priority |
|---|---|---|
| T-0240 | Fill biquad, FFT, and partitioned convolver test coverage gaps | high |
| T-0241 | Add cable_pool ping-pong 1-sample-delay invariant test | high |
| T-0242 | Migrate integration tests to shared test infrastructure | medium |
| T-0243 | Migrate DSP determinism/reset tests to shared macros | medium |
| T-0244 | Consolidate low-value tests into parameterised forms | medium |
| T-0245 | Migrate DSL tests to shared support module | low |
| T-0246 | Apply ModuleHarness helpers across patches-modules tests | low |
