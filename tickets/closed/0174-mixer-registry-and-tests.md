---
id: "0174"
title: Register mixer modules and add integration tests
priority: medium
epic: E029
created: 2026-03-20
depends_on:
  - "0170"
  - "0171"
  - "0172"
  - "0173"
---

## Summary

Wire the four mixer types into `patches_modules::default_registry()` and add
cross-crate integration tests exercising end-to-end patch assembly with a mixer
(DSL → interpreter → engine → audio tick).

## Tasks

### Registry registration

In `patches-modules/src/lib.rs`:

1. Add `pub mod mixer;` (or individual module files, depending on how the mixer
   implementations are organised — a single `mixer` submodule with four structs is
   recommended).
2. Re-export all four types at the crate root.
3. Add four `r.register::<…>()` calls in `default_registry()`.
4. Extend the `default_registry_contains_all_modules` unit test with the four new
   module names (`"Mixer"`, `"StereoMixer"`, `"PolyMixer"`, `"StereoPolyMixer"`).

### Integration tests

Add `patches-integration-tests/tests/mixer.rs`. Tests should use `HeadlessEngine`
and real `.patches` DSL strings. Suggested coverage:

- **`mono_mixer_sums_two_channels`**: 2-channel `Mixer`, both channels at unity
  level, verify output equals sum of inputs after one tick.
- **`stereo_mixer_pans_hard_left`**: `StereoMixer` with one channel panned full
  left; verify `out_left` ≈ signal, `out_right` ≈ 0.
- **`poly_mixer_sums_per_voice`**: 2-channel `PolyMixer`, check per-voice summation.
- **`mixer_solo_mutes_other_channel`**: `Mixer` with solo set on channel 0; verify
  channel 1 does not appear in the output.

The integration tests do not need to exercise sends/receives through a round-trip
DSL patch (that would require a real effects module); direct `ModuleGraph` assembly
or simple DSL strings with `Mixer` wired to `AudioOut` are sufficient.

## Acceptance criteria

- [ ] All four mixer types appear in `default_registry()`.
- [ ] `default_registry_contains_all_modules` passes with the four new names.
- [ ] At least three integration tests in `patches-integration-tests/tests/mixer.rs` pass.
- [ ] `cargo test` (workspace) passes.
- [ ] `cargo clippy` (workspace) passes.
