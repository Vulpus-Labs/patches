---
id: "0128"
title: `PolyResonantLowpass` module
priority: medium
created: 2026-03-18
epic: "E024"
depends_on: ["0126"]
---

## Summary

Implement `PolyResonantLowpass` in `patches-modules/src/poly_filter.rs` as the
first polyphonic filter module, built on the `PolyBiquad` kernel from T-0126.
This ticket also establishes the structural template that `PolyResonantHighpass`
and `PolyResonantBandpass` (T-0129) will follow.

## Acceptance criteria

- [ ] New file `patches-modules/src/poly_filter.rs` with
      `pub struct PolyResonantLowpass`, registered under the name
      `"PolyLowpass"`.

- [ ] Module fields:
      - `instance_id: InstanceId`, `descriptor: ModuleDescriptor`
      - `sample_rate: f32`
      - `cutoff: f32`, `resonance: f32`, `saturate: bool`
      - `biquad: PolyBiquad`
      - `in_audio: PolyInput`, `in_cutoff_cv: PolyInput`,
        `in_resonance_cv: PolyInput`, `out_audio: PolyOutput`

- [ ] Port descriptor:
      - inputs: `in` (Poly, index 0), `cutoff_cv` (Poly, index 1),
        `resonance_cv` (Poly, index 2)
      - outputs: `out` (Poly, index 0)

- [ ] Parameter descriptors: `cutoff` (Float, 20–20 000, default 1000),
      `resonance` (Float, 0–1, default 0), `saturate` (Bool, default false).
      Same ranges as `ResonantLowpass`.

- [ ] `prepare` initialises `PolyBiquad::new_static(...)` using coefficients
      from `compute_biquad_lowpass` at the default parameters.

- [ ] `set_ports` stores the four port fields and calls
      `recompute_static_coeffs` if neither CV input is connected (same
      transition logic as `ResonantLowpass`).

- [ ] `update_validated_parameters` updates `cutoff`, `resonance`, `saturate`
      and calls `recompute_static_coeffs` when no CV inputs are connected.

- [ ] `recompute_static_coeffs` calls `compute_biquad_lowpass` once and fans
      the result to all voices via `self.biquad.set_static(...)`.

- [ ] `process` implements two paths gated on `any_cv_connected()`:

  **Static path** (no CV):
  ```
  let audio = pool.read_poly(&self.in_audio);
  let mut out = [0.0f32; 16];
  for i in 0..16 {
      out[i] = self.biquad.tick_voice(i, audio[i], self.saturate);
  }
  self.biquad.advance_counter();
  pool.write_poly(&self.out_audio, out);
  ```

  **CV path** (at least one CV connected):
  - If `biquad.should_update()`: for each voice compute
    `effective_cutoff = (self.cutoff * cutoff_cv[i].exp2()).clamp(20.0, sr*0.499)`
    and `effective_resonance = (self.resonance + resonance_cv[i]).clamp(0.0, 1.0)`,
    then call `compute_biquad_lowpass` and `biquad.begin_ramp_voice(i, ...)`.
  - Process audio and advance counter as in the static path.
  - When a CV input is not connected its array contribution is 0.0 for all
    voices (read as a disconnected poly cable, which returns `[0.0; 16]`).

- [ ] Unit tests in `poly_filter.rs`:
      - `poly_lowpass_all_voices_pass_dc`: after 4096 silent + 4096 DC samples,
        all 16 voices output ≈ 1.0 (tolerance 0.01).
      - `poly_lowpass_all_voices_attenuate_above_cutoff`: all 16 voices produce
        peak < 0.05 at 10× the cutoff frequency.
      - `poly_lowpass_voices_are_independent_with_cv`: two voices receive
        different `cutoff_cv` values; the voice with higher cutoff CV passes
        more of a mid-frequency test tone than the voice with lower cutoff CV.
      - `poly_lowpass_static_path_when_no_cv`: with no CV connected, running
        100 samples does not call `begin_ramp_voice` (verify via field
        inspection or by asserting all deltas remain zero).

- [ ] `poly_filter` module declared in `patches-modules/src/lib.rs`;
      `PolyResonantLowpass` registered in the default registry.

- [ ] `cargo build`, `cargo test`, `cargo clippy` pass with no new warnings.

## Notes

`compute_biquad_lowpass` is re-used directly from `filter.rs` (already `pub`
within the crate). It is not duplicated.

The static path calls `advance_counter()` even though deltas are all zero.
This is harmless and keeps the counter consistent if connectivity transitions
to CV mid-playback.

`pool.read_poly` on a disconnected input returns `[0.0f32; 16]` — this is the
established poly convention and means we never need to branch on each voice
individually for missing CV.
