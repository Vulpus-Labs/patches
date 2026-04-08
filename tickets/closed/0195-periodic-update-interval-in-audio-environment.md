---
id: "0195"
title: Add periodic_update_interval to AudioEnvironment
priority: high
created: 2026-03-26
epic: E036
---

## Summary

Add `pub periodic_update_interval: u32` to `AudioEnvironment`. The field is computed in `SoundEngine::open()` as `BASE_PERIODIC_UPDATE_INTERVAL * oversampling_factor` (32, 64, 128, or 256). All hardcoded construction sites are updated to include the field; tests that don't use oversampling use a value of 32.

## Acceptance criteria

- [ ] `AudioEnvironment` has a `pub periodic_update_interval: u32` field.
- [ ] `patches-core/src/lib.rs` exports `pub const BASE_PERIODIC_UPDATE_INTERVAL: u32 = 32` (replacing or alongside `COEFF_UPDATE_INTERVAL` — the old constant may remain as an alias for now to keep downstream churn in a single ticket).
- [ ] `SoundEngine::open()` sets `periodic_update_interval = BASE_PERIODIC_UPDATE_INTERVAL * oversampling_factor`.
- [ ] All other `AudioEnvironment { .. }` construction sites (test helpers, `patches-player`, `patches-core/src/registries/`) are updated to include `periodic_update_interval: BASE_PERIODIC_UPDATE_INTERVAL` (i.e., 32, assuming no oversampling at those sites).
- [ ] `cargo build`, `cargo test`, and `cargo clippy` all pass with zero warnings.

## Notes

Construction sites identified in codebase exploration:
- `patches-engine/src/engine.rs` — primary, computed from `oversampling_factor`
- `patches-player/src/main.rs` — hardcode 32
- `patches-core/src/registries/module_builder.rs` — hardcode 32
- `patches-core/src/registries/registry.rs` — hardcode 32
- 15 test sites across `patches-integration-tests` and `patches-modules`

No module or scheduler logic changes in this ticket — those are T-0196 and T-0197.
