---
id: "0697"
title: Expander desugaring + observer manifest emission
priority: high
created: 2026-04-26
---

## Summary

After expansion and validation (tickets 0694–0696), desugar tap
targets into synthetic `~audio_tap` / `~trigger_tap` module instances
per ADR 0054 §2, assign backplane slots by global alphabetical order
per §3, and emit a `Vec<TapDescriptor>` manifest per §6. Engine-side
module implementations don't exist yet — this ticket produces the
desugared FlatPatch and manifest as data; phase 2 will wire them to
real modules.

## Acceptance criteria

- [ ] After expansion, collect all `TapTarget`s in the patch.
- [ ] Group by underlying audio-side module: everything except
  `trigger_led` → `AudioTap`; `trigger_led` → `TriggerTap`.
  Compound taps containing only audio-cable components → `AudioTap`.
  Compound taps mixing cable kinds → diagnostic (already rejected
  earlier; assert here as invariant).
- [ ] Sort all tap names globally alphabetical → assign `slot:
  usize` indices [0, N).
- [ ] Synthesise one `~audio_tap` instance with `channels = N_audio`
  if any audio taps exist; one `~trigger_tap` instance with
  `channels = N_trigger` if any trigger taps exist.
- [ ] Per-channel parameters on each synthetic instance:
  `tap_name: String`, `slot_offset: usize` (= the global slot index
  for that tap name).
- [ ] Rewrite each original cable so its destination is
  `~audio_tap.in[<tap_name>]` or `~trigger_tap.in[<tap_name>]` with
  cable gain preserved.
- [ ] Emit `Manifest = Vec<TapDescriptor>` sorted by slot, where:
  ```rust
  pub struct TapDescriptor {
      pub slot: usize,
      pub name: String,
      pub components: Vec<TapType>,
      pub params: TapParamMap,
      pub source: ProvenanceTag,
  }
  ```
  `TapParamMap` is an untyped k/v map, qualifier-resolved (so a
  qualified `meter.window` and an unqualified `window` on a
  `~meter(...)` target both surface as `("meter", "window") → value`
  or equivalent canonical key).
- [ ] `sample_rate` is *not* part of `TapDescriptor` at the DSL
  layer; the planner will inject it when building the engine. Note
  this in the type's doc comment.
- [ ] Tests: snapshot fixtures with input `.patches` and expected
  desugared FlatPatch + manifest. Cover: simple meter, compound
  meter+spectrum, mixed audio + trigger taps (separate synthetic
  modules), alphabetical sort across tap types, cable gain
  preservation.
- [ ] Synthetic tap modules will fail interpreter validation at this
  point (no real `AudioTap` registered). The test harness should
  bypass the interpreter or use a stub registry; do not block on
  phase 2.
- [ ] `cargo test -p patches-dsl` green.

## Notes

Manifest type lives in `patches-dsl` for now (the producer). Phase 2
may move it to a shared crate (`patches-observation` or
`patches-core`) once consumers exist. Keep it self-contained so the
move is a rename, not a redesign.

The synthetic module name (`~audio_tap`) is a literal identifier — it
uses the reserved `~` prefix that user code can't write. Document
this in the type that emits it so future readers know why it's
allowed.

Provenance tags should point at the original `~` site of each tap
target, so observer-side errors can be navigated back to the user's
source.

## Cross-references

- ADR 0054 §§2, 3, 6 — desugaring, slot ordering, manifest shape.
- E118 — parent epic.
