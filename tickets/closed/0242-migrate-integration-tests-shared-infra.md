---
id: "0242"
title: Migrate integration tests to shared test infrastructure
priority: medium
created: 2026-04-01
---

## Summary

The shared test infrastructure in `patches-integration-tests/src/lib.rs` now
provides `ImpulseSource`, `ConstSource`, `SineSource`, `p()`, `pi()`, `env()`,
`build_engine()`, `run_n_left()`, `run_n_stereo()`, and common constants. Each
integration test file should be migrated to use these instead of its local
copies.

## Acceptance criteria

For each file below, remove the local duplicates and import from `lib.rs`:

- [ ] `tests/delay_modules.rs` — remove local `ImpulseSource`, `ConstSource`,
      `p()`, `build_engine_and_registry()`, `run_n()`, `POOL_CAP`, `MODULE_CAP`,
      `SR`, `ENV`. Use `build_engine()` / `run_n_stereo()` from lib.
- [ ] `tests/fdn_reverb.rs` — remove local `ImpulseSource`, `p()`, `env()`,
      `POOL_CAP`, `MODULE_CAP`, `SAMPLE_RATE`.
- [ ] `tests/limiter.rs` — remove local `SineSource`, `p()`, `build_engine()`,
      `run_n()`, `POOL_CAP`, `MODULE_CAP`, `SR`, `ENV`.
- [ ] `tests/mixer.rs` — remove local `p()`, `pi()`, `env()`, `POOL_CAP`,
      `MODULE_CAP`.
- [ ] `tests/oversampling.rs` — remove local `POOL_CAP`, `MODULE_CAP` and
      use `env_at()` from lib.
- [ ] `tests/planner_v2.rs` — remove local `p()`, `env()`, `POOL_CAP`,
      `MODULE_CAP`, `SAMPLE_RATE`.
- [ ] `tests/poly_cables.rs` — remove local `p()`, `env()`, `POOL_CAP`,
      `MODULE_CAP`. Keep `PolyProbe`/`PolySource` in-file (they are
      test-specific).
- [ ] `tests/interval_scaling.rs` — use `env_at()` from lib; remove local
      `POOL_CAP`, `MODULE_CAP`. Keep `PeriodicCounter` in-file.
- [ ] `tests/sah_quant.rs` — no changes needed (already uses `ModuleHarness`).
- [ ] All tests pass, zero clippy warnings.

## Notes

Some files define specialised helpers (e.g. `limiter_params()`,
`delay_param_map()`) that are test-specific and should stay local. Only
migrate the duplicated infrastructure.

Where a file uses a different sample rate (e.g. limiter at 48000), use
`env_at(48_000.0)` and `build_engine_with()`.
