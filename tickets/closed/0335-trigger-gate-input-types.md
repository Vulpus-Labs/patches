---
id: "0335"
title: Define TriggerInput, PolyTriggerInput, GateInput, PolyGateInput types
priority: high
created: 2026-04-12
---

## Summary

Add four new edge-detecting input types to `patches-core/src/cables.rs`,
alongside the existing `MonoInput`/`PolyInput` types. These bundle cable
reading with rising-edge / gate-level detection, eliminating the manual
`prev_*` boilerplate repeated across many modules.

## Acceptance criteria

- [ ] `TriggerInput` wraps `MonoInput` + `prev: f32`; `tick(&mut self, pool) -> bool` returns true on rising edge (crosses 0.5 upward); `value() -> f32` exposes the last-read raw value
- [ ] `PolyTriggerInput` wraps `PolyInput` + `prev: [f32; 16]`; `tick` returns `[bool; 16]`; `values() -> [f32; 16]`
- [ ] `GateEdge` struct: `{ rose: bool, fell: bool, is_high: bool }`, derives `Copy`
- [ ] `GateInput` wraps `MonoInput` + `prev: f32`; `tick` returns `GateEdge`
- [ ] `PolyGateInput` wraps `PolyInput` + `prev: [f32; 16]`; `tick` returns `[GateEdge; 16]`
- [ ] All types implement `Default`, `from_ports`, `is_connected`
- [ ] All types re-exported from `patches-core/src/lib.rs`
- [ ] Unit tests covering: no edge on first call below threshold, rising edge on 0→1 transition, no re-trigger when held high, falling edge for gate types
- [ ] `cargo test -p patches-core` passes
- [ ] `cargo clippy -p patches-core` clean

## Notes

See ADR 0030. Epic E062.
