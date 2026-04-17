---
id: "0530"
title: Split patches-core cables/mod.rs by port kind
priority: medium
created: 2026-04-17
epic: E090
---

## Summary

[patches-core/src/cables/mod.rs](../../patches-core/src/cables/mod.rs)
is 556 lines collecting `CableKind`, `PolyLayout`, `CableValue`, and
six parallel port-type pairs: `MonoInput`/`MonoOutput`,
`PolyInput`/`PolyOutput`, `TriggerInput`/`PolyTriggerInput`,
`GateInput`/`PolyGateInput` (plus `GateEdge`), and the `InputPort` /
`OutputPort` enums that sum over them. Tests live in the sibling
`tests.rs`.

## Acceptance criteria

- [ ] Convert to `cables/mod.rs` + sibling submodules:
      `mono.rs` (Mono input/output),
      `poly.rs` (Poly input/output + PolyLayout),
      `trigger.rs` (Trigger + PolyTrigger inputs),
      `gate.rs` (Gate + PolyGate inputs + GateEdge),
      `ports.rs` (InputPort / OutputPort enums + their impls).
- [ ] `CableKind`, `CableValue` stay in `mod.rs` (foundational enums).
- [ ] Existing `mod tests;` declaration and all `pub` re-exports
      preserved so `patches_core::cables::*` paths unchanged.
- [ ] `mod.rs` under ~150 lines.
- [ ] `cargo build -p patches-core`, `cargo test -p patches-core`,
      `cargo clippy` clean workspace-wide.

## Notes

E090. Largest remaining split-candidate in `patches-core`. This file
is touched by nearly every consumer — keep re-exports exact.
