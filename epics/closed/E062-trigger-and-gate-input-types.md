---
id: "E062"
title: Trigger and gate input types
created: 2026-04-12
tickets: ["0335", "0336", "0337", "0338", "0339", "0340", "0341", "0342"]
---

## Summary

Replace the repeated trigger rising-edge and gate detection boilerplate
across modules with dedicated input types: `TriggerInput`,
`PolyTriggerInput`, `GateInput`, and `PolyGateInput`. These wrap the
existing `MonoInput`/`PolyInput` types with built-in edge-detection state,
providing a single-call `tick(pool)` interface.

Also refactor `AdsrCore` to accept bools instead of raw floats, moving edge
detection to the module layer, and standardise the LFO sync input on the
0.5 threshold convention.

See ADR 0030 for the design rationale.

## Tickets

1. **0335** — Define new types in `patches-core`
2. **0336** — Refactor `AdsrCore` to accept bools
3. **0337** — Refactor `Adsr` and `PolyAdsr` modules
4. **0338** — Refactor drum modules (kick, snare, hihat, cymbal, claves, clap_drum, tom)
5. **0339** — Refactor sah and poly_sah
6. **0340** — Refactor seq
7. **0341** — Refactor LFO sync input
8. **0342** — Refactor `DecayEnvelope`, `PitchSweep`, `BurstNoise` to accept bool triggers
