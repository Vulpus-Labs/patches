---
id: "0126"
title: Implement `PolyBiquad` kernel in `common::poly_biquad`
priority: high
created: 2026-03-18
epic: "E024"
depends_on: ["0125"]
---

## Summary

Implement the polyphonic biquad kernel in
`patches-modules/src/common/poly_biquad.rs`. The design centres on a
`VoiceFilter` struct that packs all per-voice hot state — active coefficients,
interpolation deltas, and filter memory — into 48 bytes (one cache line).
`PolyBiquad` holds a `[VoiceFilter; 16]` for per-sample processing and five
cold `[f32; 16]` arrays for target coefficients accessed only at update
boundaries.

This ticket delivers the kernel only; no `Module` implementations use it yet
(those are T-0128 and T-0129).

## Acceptance criteria

- [ ] `patches-modules/src/common/poly_biquad.rs` defines:

  ```rust
  #[repr(C)]
  pub(crate) struct VoiceFilter {
      // Active biquad coefficients
      pub b0: f32, pub b1: f32, pub b2: f32, pub a1: f32, pub a2: f32,
      // Per-sample coefficient deltas (CV interpolation path)
      pub db0: f32, pub db1: f32, pub db2: f32, pub da1: f32, pub da2: f32,
      // TDFII filter memory
      pub s1: f32, pub s2: f32,
  }
  ```

  `size_of::<VoiceFilter>()` is asserted to equal 48 via a compile-time
  `const _: () = assert!(...)` or a `#[test]`.

- [ ] `pub(crate) struct PolyBiquad` contains:
      - `voices: [VoiceFilter; 16]` — hot per-voice state
      - `b0t: [f32; 16]`, `b1t: [f32; 16]`, `b2t: [f32; 16]`,
        `a1t: [f32; 16]`, `a2t: [f32; 16]` — cold target coefficients
      - `update_counter: u32`

- [ ] `PolyBiquad::new_static(b0, b1, b2, a1, a2) -> Self` initialises all 16
      `VoiceFilter` entries with the given coefficients, zeros all deltas and
      state, fans the same values into all five target arrays, and sets
      `update_counter = 0`.

- [ ] `PolyBiquad::set_static(b0, b1, b2, a1, a2)` fans the given coefficients
      into every `VoiceFilter`'s active and target fields, zeros all deltas.
      Does not touch `s1`/`s2` or `update_counter`.

- [ ] `PolyBiquad::should_update(&self) -> bool` returns `true` when
      `update_counter == 0`.

- [ ] `PolyBiquad::begin_ramp_voice(&mut self, i: usize, b0t, b1t, b2t, a1t, a2t)`
      snaps `voices[i]`'s active coefficients to the corresponding target
      arrays (`self.b0t[i]`, etc.), stores the new targets, and computes deltas
      as `(new_target - snapped_active) * COEFF_UPDATE_INTERVAL_RECIPROCAL`.
      The owning module calls this for each voice at update boundaries after
      computing per-voice effective parameters.

- [ ] `PolyBiquad::tick_voice(&mut self, i: usize, x: f32, saturate: bool) -> f32`
      runs one sample of the TDFII recurrence for voice `i`, advances that
      voice's active coefficients by their deltas, and returns `y`. Does not
      touch the `update_counter` — the owning module advances it once after
      processing all voices via `PolyBiquad::advance_counter(&mut self)`, which
      increments and wraps at `COEFF_UPDATE_INTERVAL`.

- [ ] `PolyBiquad` is re-exported from `crate::common`.

- [ ] Unit tests in `poly_biquad.rs`:
      - `voice_filter_size_is_48`: asserts `size_of::<VoiceFilter>() == 48`.
      - `set_static_fans_out_to_all_voices`: after `set_static`, every voice
        has the given active coefficients and zero deltas.
      - `begin_ramp_voice_snaps_then_ramps`: after `begin_ramp_voice(0, ...)`,
        `voices[0]` has deltas proportional to `(target - snapped) /
        COEFF_UPDATE_INTERVAL` and other voices are untouched.
      - `tick_voice_advances_deltas`: after `begin_ramp_voice` followed by one
        `tick_voice`, the active coefficient has moved by one delta step.
      - `voices_are_independent`: two voices initialised with different
        coefficients produce different outputs from `tick_voice` on the same
        input.

- [ ] `cargo build`, `cargo test`, `cargo clippy` pass with no new warnings.

## Notes

`COEFF_UPDATE_INTERVAL_RECIPROCAL` is re-used from `common::mono_biquad`
(made `pub(crate)` in T-0125).

The `voices` array is iterated sequentially in `process` by the owning module
(`for i in 0..16 { ... }`). No SIMD vectorisation is attempted here; the
layout is designed to make auto-vectorisation possible in future if the compiler
can see the tight loop.

Target arrays (`b0t` etc.) are kept outside `VoiceFilter` deliberately: their
presence would push the struct from 48 to 88 bytes, straddling two cache lines
and polluting the hot loop. The cold access pattern (once per 32 samples) means
non-sequential access of the five arrays is not a concern.
