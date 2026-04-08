---
id: "E035"
title: SAH, PolySAH, Quant, PolyQuant modules
status: open
priority: medium
created: 2026-03-25
tickets:
  - "0190"
  - "0191"
  - "0192"
  - "0193"
  - "0194"
---

## Summary

Adds four new modules to `patches-modules` supporting the workflow:

```text
Noise ──► SAH ──► Quant
          ▲
        Clock
```

Random noise is sampled on each clock trigger, then snapped to the nearest pitch
within a user-supplied note set, producing a melodic sequence driven by a clock.

| Module      | Kind | Role                                  |
|-------------|------|---------------------------------------|
| `Sah`       | Mono | Sample-and-hold (rising-edge trigger) |
| `PolySah`   | Poly | Polyphonic sample-and-hold            |
| `Quant`     | Mono | v/oct quantiser to a note array       |
| `PolyQuant` | Poly | Polyphonic quantiser                  |

---

## `Sah` — mono sample-and-hold

On each rising edge of `trig` (signal crossing 0.5 from below), the current
value of `in` is latched and held on `out` until the next trigger.

**Ports:**

- `in` (MonoInput) — signal to sample
- `trig` (MonoInput) — trigger/gate input
- `out` (MonoOutput) — held output

**Parameters:** none

**State:** `held: f32`, `prev_trig: f32`

---

## `PolySah` — polyphonic sample-and-hold

Same behaviour as `Sah` across 16 voices. A single mono `trig` is broadcast to
all voices so they latch simultaneously.

**Ports:**

- `in` (PolyInput)
- `trig` (MonoInput)
- `out` (PolyOutput)

---

## `Quant` — mono quantiser

Quantises a continuous v/oct signal to the nearest note in a user-supplied set.
Always free-running: re-quantises every sample.

**Ports:**

- `in` (MonoInput) — continuous v/oct
- `out` (MonoOutput) — quantised and transformed v/oct
- `trig_out` (MonoOutput) — emits `1.0` for exactly one sample when the
  quantised pitch changes, otherwise `0.0`

**Parameters:**

- `notes` (Array, max 12) — semitone offsets within an octave, e.g.
  `["0", "2", "4", "7", "9"]` for pentatonic; default `["0"]`.
  Values outside `[0, 11]` are clamped. If empty after parsing, treated as `[0]`.
- `centre` (Float, [−4.0, 4.0], default `0.0`) — added to the transformed output
- `scale` (Float, [−4.0, 4.0], default `1.0`) — scales the raw quantised v/oct

**Transform:** `out = centre + (quantised_voct * scale)`

**Algorithm:**

1. `octave = floor(in)`, `semitone_frac = (in - octave) * 12.0`
2. For each note `n` in the sorted notes array, compute circular distance to
   `semitone_frac` (considering the gap between `notes.last()` and `12 + notes[0]`)
3. Choose the nearest note; if nearest is across the octave boundary, adjust
   `octave ± 1`
4. `quantised_voct = octave + (nearest_note / 12.0)`
5. `out = centre + (quantised_voct * scale)`

**Internal state (all pre-allocated, no allocations in `process`):**

- `notes_buf: [f32; 12]`, `notes_len: usize` — sorted semitone fractions
- `last_quantised: f32` — for change detection / `trig_out`

---

## `PolyQuant` — polyphonic quantiser

Applies `Quant` logic independently to each of 16 voices. Always free-running.

**Ports:**

- `in` (PolyInput)
- `out` (PolyOutput)
- `trig_out` (PolyOutput) — per-voice pitch-change pulse

**Parameters:** identical to `Quant`

**State:** `notes_buf`, `notes_len` shared; `last_quantised: [f32; 16]` per voice

---

## Tickets

- [T-0190](../tickets/open/0190-sah-module.md) — `Sah` module
- [T-0191](../tickets/open/0191-poly-sah-module.md) — `PolySah` module
- [T-0192](../tickets/open/0192-quant-module.md) — `Quant` module
- [T-0193](../tickets/open/0193-poly-quant-module.md) — `PolyQuant` module
- [T-0194](../tickets/open/0194-sah-quant-registry-and-tests.md) — Register all
  four; integration tests
