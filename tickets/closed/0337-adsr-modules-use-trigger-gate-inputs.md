---
id: "0337"
title: Refactor Adsr and PolyAdsr to use TriggerInput and GateInput
priority: high
created: 2026-04-12
---

## Summary

Update the `Adsr` and `PolyAdsr` modules to use `TriggerInput`/`GateInput`
(and poly variants) instead of raw `MonoInput`/`PolyInput` for their trigger
and gate ports. Pass the edge/level results into the updated
`AdsrCore::tick(bool, bool)`.

## Acceptance criteria

- [ ] `Adsr`: trigger port uses `TriggerInput`, gate port uses `GateInput`
- [ ] `Adsr::process` calls `core.tick(self.in_trigger.tick(pool), self.in_gate.tick(pool).is_high)`
- [ ] `PolyAdsr`: trigger port uses `PolyTriggerInput`, gate port uses `PolyGateInput`
- [ ] `PolyAdsr::process` passes per-voice bools from poly tick results into per-voice `AdsrCore::tick`
- [ ] `cargo test -p patches-modules` passes
- [ ] `cargo clippy -p patches-modules` clean

## Notes

Depends on 0335 and 0336. See ADR 0030. Epic E062.
