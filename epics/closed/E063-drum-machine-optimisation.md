---
id: "E063"
title: Drum machine optimisation
created: 2026-04-12
tickets: ["0343", "0344", "0345", "0346", "0347"]
---

## Summary

Performance and API improvements to the drum DSP primitives and drum
modules. These are separate from the trigger/gate input type refactor
(E062), though some tickets may be sequenced after E062 changes land.

## Tickets

1. **0343** — Separate PitchSweep configuration from triggering
2. **0344** — Separate MetallicTone configuration from triggering
3. **0345** — Cache sr_recip and use fast_sine in MetallicTone
4. **0346** — Remove redundant set_decay calls from drum trigger blocks
5. **0347** — BurstGenerator set_params should accept time in seconds
