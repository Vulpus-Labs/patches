---
id: "E116"
title: Test assertion hardening — close silent-pass gaps
created: 2026-04-24
tickets: ["0675", "0676", "0677", "0678", "0679"]
adrs: []
---

## Goal

A repo-wide audit identified weak-assertion patterns that allow real
regressions to pass silently. The audit also confirmed the test suite
is mostly sound — `patches-dsp` and `patches-dsl` are clean, and the
2:1 test-to-impl ratio in core crates tracks reasonable engineering.
This epic fixes the specific clusters of weakness, without broader
churn.

## Identified patterns

1. **Bare `.is_ok()` / "doesn't panic" success paths** — build succeeds
   → no inspection of result shape. Concentrated in patches-engine,
   patches-interpreter, patches-host, patches-integration-tests.
2. **RMS-only output checks on drum modules** — `cymbal`, `tom`,
   `hihat` assert `rms > threshold` over a tick window; DC offset,
   constant noise, wrong pitch, or wrong envelope would all pass.
3. **String-substring goldens in LSP and SVG** — `s.contains("x=4")`
   passes silently on format changes. Both subsystems produce
   deterministic textual output; ideal for `insta` snapshots.
4. **Magic tolerance bands in patches-vintage** — unexplained
   thresholds (`< 0.5 * fundamental`, `> 85% of naive baseline`,
   `< 0.1 peak`) could hide factor-of-10 regressions.

## Non-goals

- Descriptor-shape tautologies (midi_transpose etc.) and sentinel mock
  tests (lfo disconnect pool) — low harm, not worth a sweep.
- Binary-golden regeneration workflow (fdn_reverb, vintage_baseline) —
  rare path; revisit if it bites.
- Adding test coverage to currently untested crates (patches-cpal,
  patches-profiling, etc.) — separate concern.

## Tickets

- 0675 — Drum module spectral and envelope assertions
- 0676 — patches-svg snapshot tests via `insta`
- 0677 — Replace bare `.is_ok()` with shape assertions (engine,
  interpreter, host, integration)
- 0678 — patches-lsp hover/inlay snapshot tests
- 0679 — Document or tighten patches-vintage tolerance bands
