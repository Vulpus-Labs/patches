---
id: "E024"
title: Biquad filter suite — shared kernels and mono/poly LP/HP/BP modules
created: 2026-03-18
tickets: ["0125", "0126", "0127", "0128", "0129", "0130"]
---

## Summary

The existing `ResonantLowpass` module contains its biquad processing logic
inline. This epic extracts that logic into reusable `MonoBiquad` and `PolyBiquad`
kernels, then builds a full set of mono and polyphonic filter modules on top:
lowpass, highpass, and bandpass in both voicings.

The only difference between filter topologies is the coefficient formula.
The TDFII recurrence, the coefficient-interpolation scheme (snap → ramp over
`COEFF_UPDATE_INTERVAL` samples), and the static/CV branching logic are
identical across all six filter types and live once in the kernel.

The `PolyBiquad` kernel follows the `VoiceFilter`-based layout designed prior
to implementation: active coefficients, per-sample deltas, and filter memory
for all 16 voices are packed into a `[VoiceFilter; 16]` array (48 bytes per
voice, one cache line) so the inner loop touches a single contiguous block of
memory per voice. Cold target coefficients live in five separate `[f32; 16]`
arrays accessed only at update boundaries.

## Tickets

| ID   | Title                                                          | Priority | Depends on  |
|------|----------------------------------------------------------------|----------|-------------|
| 0125 | Extract `MonoBiquad` kernel from `ResonantLowpass`            | high     | —           |
| 0126 | Implement `PolyBiquad` kernel in `common::poly_biquad`        | high     | 0125        |
| 0127 | Mono `ResonantHighpass` and `ResonantBandpass` filters        | medium   | 0125        |
| 0128 | `PolyResonantLowpass` module                                  | medium   | 0126        |
| 0129 | `PolyResonantHighpass` and `PolyResonantBandpass` modules     | medium   | 0128        |
| 0130 | Register all new filters; end-to-end integration test         | medium   | 0127, 0129  |

## Definition of done

- `MonoBiquad` and `PolyBiquad` kernels live in `patches-modules/src/common/`
  with no module-level coupling (they carry no port fields or parameter maps).
- `ResonantLowpass` is refactored to delegate entirely to `MonoBiquad`;
  all existing tests pass unchanged.
- `ResonantHighpass`, `ResonantBandpass`, `PolyResonantLowpass`,
  `PolyResonantHighpass`, and `PolyResonantBandpass` are implemented and
  registered in the default module registry.
- Each new mono filter has unit tests covering DC pass/reject, frequency
  response shape, and CV modulation behaviour.
- Each new poly filter has unit tests covering correct per-voice independence
  and the static/CV path boundary.
- `cargo build`, `cargo test`, `cargo clippy` pass with no warnings across all
  crates.
- No `unwrap()` or `expect()` in library code.
