---
id: "E061"
title: Electronic drum sound modules (808-ish)
created: 2026-04-12
tickets: ["0327", "0328", "0329", "0330", "0331", "0332", "0333", "0334"]
---

## Summary

Add a suite of self-contained electronic drum modules, each producing a
complete drum sound from a single trigger input. The synthesis flavour is
broadly 808-inspired but prioritises parameterisability over authenticity —
every module exposes enough knobs to cover a wide timbral range.

Each module is fully integrated: oscillators, noise sources, envelopes, and
filters are internal. No external ADSR, noise, or VCA is needed. The only
required input is a trigger signal; optional CV inputs allow per-hit
modulation of key parameters.

New DSP primitives (decay envelopes, pitch sweeps, waveshaping, metallic
tone generation) live in `patches-dsp` with unit tests. Module
implementations live in `patches-modules`.

## Modules

| Module       | Core synthesis                                                  |
| ------------ | --------------------------------------------------------------- |
| Kick         | Sine osc + pitch envelope + amp envelope + optional saturation  |
| Snare        | Tuned body (sine/tri) + filtered noise burst, separate envelopes |
| Clap         | Filtered noise with retriggered bursts then decay               |
| ClosedHiHat  | Metallic tone (inharmonic square oscs) + HP noise, short decay  |
| OpenHiHat    | Same engine as closed, longer decay, separate choke input       |
| Tom          | Sine osc + pitch envelope + slight noise layer                  |
| Claves       | High-pitched resonant bandpass tone, sharp attack               |
| Cymbal       | Metallic tones + highpass noise, long decay with shimmer        |

## Tickets

| ID   | Title                                          | Priority | Depends on |
| ---- | ---------------------------------------------- | -------- | ---------- |
| 0327 | Drum synthesis DSP primitives in patches-dsp   | high     | —          |
| 0328 | Kick drum module                               | high     | 0327       |
| 0329 | Snare drum module                              | high     | 0327       |
| 0330 | Clap module                                    | medium   | 0327       |
| 0331 | Hi-hat modules (open + closed)                 | medium   | 0327       |
| 0332 | Tom module                                     | medium   | 0327, 0328 |
| 0333 | Claves module                                  | medium   | 0327       |
| 0334 | Cymbal module                                  | medium   | 0327, 0331 |

## Definition of done

- All eight drum modules are registered in `default_registry()`.
- Each module produces sound from a trigger input alone — no external
  wiring required beyond trigger.
- Kick module accepts a `pitch` parameter (Hz or V/Oct) and produces
  pitched output.
- DSP primitives in `patches-dsp` have unit tests covering core behaviour
  (envelope shape, pitch sweep accuracy, waveshaper symmetry).
- Each module has tests exercising trigger response and basic output.
- Module doc comments follow the standard format (inputs/outputs/parameters
  tables).
- `cargo build`, `cargo test`, `cargo clippy` pass with no warnings.
- No `unwrap()` or `expect()` in library code.
