# ADR 0030 — Trigger and gate input types

**Date:** 2026-04-12
**Status:** accepted

---

## Context

The rising-edge detection pattern for trigger inputs is duplicated across
many modules:

```rust
// In struct:
prev_trigger: f32,
in_trigger: MonoInput,

// In process:
let trigger = pool.read_mono(&self.in_trigger);
let trigger_rose = trigger >= 0.5 && self.prev_trigger < 0.5;
self.prev_trigger = trigger;
if trigger_rose { /* ... */ }
```

This appears in all drum modules (kick, snare, hihat, cymbal, claves,
clap_drum, tom), the sequencer (four independent triggers), sample-and-hold,
and indirectly in AdsrCore. Gate detection (`gate >= 0.5`) follows a similar
pattern in ADSR and sequencer modules.

The duplication creates several problems:

1. **Boilerplate** — every triggered module carries a `prev_*: f32` field
   and a 3-line read/detect/update block.
2. **Inconsistency risk** — the LFO sync input currently uses a different
   threshold convention (`<= 0.0` / `> 0.0`) for the same logical operation.
3. **Mixed concerns** — `AdsrCore` in `patches-dsp` performs its own edge
   detection internally, meaning trigger semantics are split between the
   module layer and the DSP layer.

---

## Decision

### New input types in `patches-core/src/cables.rs`

Four new types are added alongside `MonoInput`, `PolyInput`, `MonoOutput`,
and `PolyOutput`:

| Type               | Wraps       | State            | `tick` returns   |
|--------------------|-------------|------------------|------------------|
| `TriggerInput`     | `MonoInput` | `prev: f32`      | `bool`           |
| `PolyTriggerInput` | `PolyInput` | `prev: [f32;16]` | `[bool; 16]`     |
| `GateInput`        | `MonoInput` | `prev: f32`      | `GateEdge`       |
| `PolyGateInput`    | `PolyInput` | `prev: [f32;16]` | `[GateEdge; 16]` |

`GateEdge` is a small `Copy` struct: `{ rose: bool, fell: bool, is_high: bool }`.

Each type wraps the corresponding plain input as a public `inner` field and
adds edge-detection state. Construction mirrors the existing types:

```rust
self.in_trigger = TriggerInput::from_ports(inputs, idx);
```

Usage in `process`:

```rust
if self.in_trigger.tick(pool) {
    // rising edge — fire
}
// If the raw value is still needed (e.g. for envelope retrigger):
let trigger_val = self.in_trigger.value();
```

### Threshold convention: 0.5

All trigger and gate types use the same threshold: `>= 0.5` is high,
`< 0.5` is low. This is already the convention in all modules except the
LFO sync input, which is updated to match.

### Standard for new modules

**All new modules that respond to triggers or gates must use `TriggerInput` /
`GateInput` (or their poly variants) rather than raw `MonoInput` with manual
edge detection.** This keeps trigger semantics in one place and ensures
consistent threshold behaviour across the system.

### AdsrCore takes bools, not floats

`AdsrCore::tick` changes from `tick(trigger: f32, gate: f32)` to
`tick(triggered: bool, gate_high: bool)`. Edge detection is the caller's
responsibility, performed via `TriggerInput` and `GateInput` at the module
layer. This removes the internal `prev_trigger` field from `AdsrCore` and
cleanly separates DSP (envelope state machine) from signal conventions
(threshold, edge detection).

---

## Consequences

- Modules that use triggers or gates become shorter and more uniform.
- The 0.5 threshold is defined in exactly one place.
- `AdsrCore` becomes a pure state machine driven by boolean events,
  making it easier to test and reuse outside the module system.
- The `prev` state in `TriggerInput`/`GateInput` is reset when `set_ports`
  is called (cable reconnection), which is correct — reconnection should
  not carry stale edge-detection state.
- `TriggerInput` et al. do not derive `Copy` or `PartialEq` because they
  carry mutable runtime state, unlike the plain input types which are
  purely configurational.
