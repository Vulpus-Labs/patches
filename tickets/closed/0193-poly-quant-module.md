---
id: "0193"
title: "`PolyQuant` module (polyphonic v/oct quantiser)"
priority: medium
created: 2026-03-25
epic: "E035"
---

## Summary

Implement `PolyQuant` in `patches-modules/src/poly_quant.rs`. Applies the same
quantisation logic as `Quant` (T-0192) independently to each of 16 voices, with
a single mono `trig` gating all voices simultaneously.

## Acceptance criteria

- [ ] `patches-modules/src/poly_quant.rs` defines `pub struct PolyQuant`.

### Descriptor

- [ ] `PolyQuant::describe` produces:
  - `poly_in("in")` — poly v/oct
  - `poly_out("out")` — quantised and transformed poly v/oct
  - `poly_out("trig_out")` — per-voice pitch-change pulse
  - `array_param("notes", &["0"], 12)`
  - `float_param("centre", -4.0, 4.0, 0.0)`
  - `float_param("scale", -4.0, 4.0, 1.0)`

### State

- [ ] Pre-allocated internal state (no heap allocation in `process`):
  - `notes_buf: [f32; 12]`, `notes_len: usize` — shared across voices
  - `last_quantised: [f32; 16]` — per-voice, initialised to `0.0`
  - `pending_trig_out: [f32; 16]` — per-voice one-sample pulse
  - `centre: f32`, `scale: f32`

### Parameter update

- [ ] Same parsing logic as `Quant::update_validated_parameters`.

### Port assignment

- [ ] `set_ports` assigns `in_sig: PolyInput`, `out: PolyOutput`,
  `trig_out: PolyOutput`.

### Processing

- [ ] `process` logic:
  1. `voices = pool.read_poly(&self.in_sig)` — `[f32; 16]`
  2. For each voice `i`:
     - Apply the same nearest-note search as `Quant`; compute `new_quant`.
     - If `(new_quant - self.last_quantised[i]).abs() > 1e-6`, set
       `self.pending_trig_out[i] = 1.0` and `self.last_quantised[i] = new_quant`.
  3. Build `out_buf: [f32; 16]` where
     `out_buf[i] = self.centre + self.last_quantised[i] * self.scale`.
  4. Write `pool.write_poly(&self.out, out_buf)`.
  5. Write `pool.write_poly(&self.trig_out, self.pending_trig_out)`.
  6. `self.pending_trig_out = [0.0; 16]` (reset all pulses).

- [ ] No `unwrap()` / `expect()`.
- [ ] `cargo clippy` clean, all existing tests pass.

## Notes

The nearest-note search can be extracted into a private free function
`quantise_note(semitone_frac: f32, notes: &[f32]) -> (f32, i32)` returning
`(nearest_semitone, octave_adj)` and shared between `Quant` and `PolyQuant` via
a common helper module `patches-modules/src/quant_util.rs` (or inlined if the
duplication is small).

Export from `lib.rs`, do not register yet (T-0194).
