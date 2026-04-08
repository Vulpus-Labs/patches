---
id: "0192"
title: "`Quant` module (mono v/oct quantiser)"
priority: medium
created: 2026-03-25
epic: "E035"
---

## Summary

Implement `Quant` in `patches-modules/src/quant.rs`. Snaps a continuous v/oct
signal to the nearest pitch in a user-supplied semitone set, then applies a
`centre + (quantised_voct * scale)` transform. Emits a one-sample pulse on
`trig_out` whenever the quantised pitch changes.

## Acceptance criteria

- [ ] `patches-modules/src/quant.rs` defines `pub struct Quant`.

### Descriptor

- [ ] `Quant::describe` produces:
  - `mono_in("in")` — continuous v/oct
  - `mono_out("out")` — quantised and transformed v/oct
  - `mono_out("trig_out")` — pitch-change pulse
  - `array_param("notes", &["0"], 12)` — semitone offsets, default is root only
  - `float_param("centre", -4.0, 4.0, 0.0)`
  - `float_param("scale", -4.0, 4.0, 1.0)`

### State

- [ ] Pre-allocated internal state (no heap allocation in `process`):
  - `notes_buf: [f32; 12]` — sorted semitone values (0.0..11.0)
  - `notes_len: usize` — number of active notes
  - `last_quantised: f32` — previous quantised voct for change detection
  - `centre: f32`, `scale: f32`
  - `pending_trig_out: f32` — set to 1.0 when a change is detected; cleared after one sample

### Parameter update

- [ ] `update_validated_parameters` parses `notes` array:
  - For each string element, parse as `i64`; clamp to `[0, 11]`; store as
    `f32` in `notes_buf`.
  - If the resulting `notes_len == 0`, treat as `[0]`.
  - Sort `notes_buf[..notes_len]` in ascending order.
  - Update `centre` and `scale`.

### Port assignment

- [ ] `set_ports` assigns `in_sig: MonoInput`, `out: MonoOutput`,
  `trig_out: MonoOutput`.

### Processing

- [ ] `process` logic (no allocations):
  1. `x = pool.read_mono(&self.in_sig)`
  2. `octave = x.floor()`, `semitone_frac = (x - octave) * 12.0`
  3. Find the nearest note in `notes_buf[..notes_len]` using circular
     distance (wrap-around at 12): for each note `n`, distance is
     `min((semitone_frac - n).abs(), 12.0 - (semitone_frac - n).abs())`.
     On a tie, prefer the lower note.
  4. Compute `new_quant = octave + (nearest / 12.0)`, adjusting `octave`
     by ±1 if the nearest note wraps across the octave boundary.
  5. If `(new_quant - self.last_quantised).abs() > 1e-6`, set
     `self.pending_trig_out = 1.0` and `self.last_quantised = new_quant`.
  6. Write `pool.write_mono(&self.out, self.centre + self.last_quantised * self.scale)`.
  7. Write `pool.write_mono(&self.trig_out, self.pending_trig_out)`.
  8. `self.pending_trig_out = 0.0` (pulse lasts exactly one sample).

- [ ] No `unwrap()` / `expect()`.
- [ ] `cargo clippy` clean, all existing tests pass.

## Notes

**Circular nearest-note search:** when `semitone_frac` is between the last note
and the first note (wrapping through 0/12), the nearest may be in the adjacent
octave. E.g. for notes `[0, 7]` and input semitone 11.5, the nearest is note 0
in the *next* octave (+1 octave adjustment). Concrete algorithm:

```
best_dist = f32::MAX
best_note = notes_buf[0]
best_octave_adj = 0
for n in notes_buf[..notes_len]:
    d_fwd = (semitone_frac - n + 12.0) % 12.0   // n is above or at frac
    d_bwd = (n - semitone_frac + 12.0) % 12.0   // n is below or at frac
    dist = d_fwd.min(d_bwd)
    if dist < best_dist:
        best_dist = dist
        best_note = n
        // octave adjusts if the nearest path wraps
        if d_bwd < d_fwd && n > semitone_frac { best_octave_adj = -1 }
        else if d_fwd < d_bwd && n < semitone_frac { best_octave_adj = 1 }
        else { best_octave_adj = 0 }
```

Export from `lib.rs`, do not register yet (T-0194).
