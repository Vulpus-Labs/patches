---
id: "0787"
title: Migrate single-channel oscillators and envelopes to TEMPLATE
priority: high
created: 2026-05-02
epic: "E132"
adrs: ["0066"]
depends_on: ["0786"]
---

## Summary

Migrate single-channel oscillator, LFO, and envelope modules in
`patches-modules` to declare a `const TEMPLATE` and delete their
`describe()` override. Sources, modulators, envelopes — anything
that produces signal without per-channel descriptor variation.

In scope (representative): `osc`, `lfo`, `adsr`, `ar`, `noise`,
`clock`, `trigger_*`, `phasor`, `sample_and_hold`, etc. Confirm the
final list against the patches-modules audit before merging.

## Acceptance criteria

- [ ] Every in-scope module declares `const TEMPLATE`.
- [ ] Each module's `describe()` override removed.
- [ ] Descriptor output byte-identical pre/post migration (assert in
      module tests).
- [ ] `cargo test -p patches-modules` passes.
- [ ] `cargo clippy -p patches-modules` clean.

## Notes

- Mechanical change; ~12-15 modules.
- Companion tickets: 0787a (filters/effects), 0787b (utility/IO).
- Do not move structural/realtime params yet (E126 owns that).
