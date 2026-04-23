---
id: "0637"
title: Unify TriggerInput with SubTriggerInput
priority: medium
created: 2026-04-23
adr: "0047"
depends_on: ["0636"]
---

## Summary

Collapse the two trigger-input types — threshold/rising-edge
`TriggerInput` / `PolyTriggerInput` (ADR 0030) and sub-sample
`SubTriggerInput` / `PolySubTriggerInput` (ADR 0047) — into just the
sub-sample pair. All existing trigger producers emit a 1.0 pulse for
one sample and 0.0 otherwise, so they round-trip through the
sub-sample encoding (`frac = 1.0`) without behaviour change.

## Rationale

Two types for the same conceptual signal is a papercut — every new
module that takes a trigger has to pick one and reason about the
difference. Unifying on the typed (`CableKind::Trigger`) sub-sample
form gives one encoding for the whole system, enables sub-sample
timing wherever a producer can supply it (e.g. phase-accumulator
derived triggers in `VDco.reset_out`), and leaves legacy pulse
producers compatible by construction.

MIDI / CLAP / host event sources remain sample-aligned
(`frac = 1.0`); no precision lost.

## Acceptance criteria

- [ ] `patches-core::cables` exports only `SubTriggerInput` /
      `PolySubTriggerInput`. `TriggerInput` / `PolyTriggerInput` and
      `TRIGGER_THRESHOLD` removed. Cable-layer tests for the removed
      types deleted.
- [ ] Every module using the old types switches to the sub-sample
      pair. Ports previously declared `mono_in` for trigger-style
      inputs are re-declared `trigger_in` / `poly_trigger_in` in
      their `ModuleDescriptor`, so cable kind matches the typed read.
- [ ] `.tick(pool)` callsites updated: `bool` → `is_some()`; `bool`
      arrays → per-voice `is_some()`.
- [ ] Doc comments in `patches-dsp` that refer to `TriggerInput` are
      updated (cosmetic).
- [ ] `cargo test` passes across `patches-core`, `patches-dsp`,
      `patches-modules`, `patches-engine`, `patches-vintage`.
- [ ] No new ADR needed — extends ADR 0047 to the whole trigger
      surface, superseding ADR 0030's threshold convention.

## Notes

Module inventory to migrate (non-exhaustive; grep for `TriggerInput\b`
excluding `Sub`):

- `patches-modules`: `adsr`, `poly_adsr`, `sah`, `poly_sah`,
  `ms_ticker`, `clap_drum`, `claves`, `cymbal`, `hihat` (trigger +
  choke, and `PolyHihat` if any), `kick`, `snare`, `tom`.

Port-kind migration implication: any `.patches` file connecting a
non-Trigger cable into one of these renamed inputs will now fail
validation. Expected — producers of triggers already emit on
`trigger_out` ports in the standard modules. Flag any breakage.
