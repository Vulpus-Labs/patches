---
id: "0555"
title: Compander primitive (NE570-style) in patches-vintage
priority: low
created: 2026-04-18
epic: E090
depends_on: ["0552"]
---

## Summary

Add a reusable log/exp compander primitive to
`patches-vintage/src/compander/` modelling the NE570/NJM2153 2:1
class used in Dimension D, Boss CE-2, EHX Small Clone, and related
BBD effects. Not consumed by VChorus (neither Juno-60 nor Juno-106
companded their chorus — verified via service notes and
florian-anwander.de); reserved for future vintage-BBD-delay and
Dimension-D-style modules in later epics.

## Design

Two halves — `Compressor` (2:1 log encode) and `Expander` (1:2 exp
decode). NE570 topology: full-wave rectifier → one-pole averaging
filter (asymmetric attack/release) → variable-gain cell.

```rust
pub struct CompanderParams {
    pub attack_s: f32,
    pub release_s: f32,
    pub ref_level: f32,
}

impl CompanderParams {
    pub const NE570_DEFAULT: Self = /* ~5 ms attack, ~100 ms release */;
}

pub struct Compressor { /* rectifier + gain-cell state */ }
pub struct Expander   { /* mirror */ }

impl Compressor {
    pub fn new(params: CompanderParams, sample_rate: f32) -> Self;
    pub fn process(&mut self, input: f32) -> f32;
    pub fn reset(&mut self);
}
// Expander: same shape.
```

Per sample: rectify → LPF → compute gain from averaged level →
multiply. Compressor gain ∝ 1/√(level); expander gain ∝ √(level).
Round-trip (compressor → delay → expander) should be unity after
settle.

## Implementation notes

- Keep the classic 2:1 log/exp topology — the specific
  breathing/pumping character depends on the fast-rectifier +
  slow-averaging structure.
- No allocations in `process`. Scalar.
- Runtime-settable params so future consumers can match different
  chips (NE570 vs MN3102-internal compander).
- Pure DSP; not a Module.

## Acceptance criteria

- [ ] `patches-vintage/src/compander/mod.rs` implements `Compressor`
      and `Expander`.
- [ ] `NE570_DEFAULT` cites the datasheet in a comment.
- [ ] Tests: steady-sine round-trip unity within tolerance after
      settle; measured attack/release within spec; silent input →
      silent output (no latching).
- [ ] No allocations on hot path.
- [ ] `cargo clippy -p patches-vintage` and `cargo test -p patches-vintage`
      clean.

## Notes

First consumer: future vintage-BBD-delay module with Compressor
before BBD + Expander after, matching CE-2/Small-Clone topology.
