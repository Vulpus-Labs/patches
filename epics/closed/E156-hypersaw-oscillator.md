---
id: E156
title: HyperSaw oscillator (detuned saw ensemble)
status: closed
created: 2026-05-27
---

## Goal

Add a "supersaw"/JP-8000-style oscillator to the repertoire: a stack of detuned
sawtooth copies (1 centre + 8 sides, 4 below + 4 above) summed per voice, whose
slow inter-copy beating gives the wide, animated lead/pad character. Mono
(`HyperSaw`) and 16-voice (`PolyHyperSaw`) variants, sharing a fixed-point,
alloc-free per-voice core.

Design and rationale recorded in **ADR 0078**. Key decisions:

- **Fixed-point `u32` phase** (exact wrap; factored detune precompute) with
  branch-free fixed-point PolyBLEP — a deliberate departure from the f32
  `polyblep` path used by `Osc`/`PolyOsc`.
- **Control-rate increment maths.** 144 saws can't recompute frequency per
  sample; detune ratios + base increments + reciprocals are computed in
  `periodic_update` and factored (8 shared ratios + 16 per-voice bases).
- **Free-running**: per-copy phase decorrelated by `xorshift64` at construction,
  not on a (nonexistent) note event.
- **Fractional density** with symmetric pair-ordered fade-in; loudness
  normalised. Centre/side **mix** as a control-rate gain.
- **No sync, no phase mod, no drift.** FM is control-rate vibrato.
- **Voice-batched kernel** (16-wide, copy-major) so the *voice* axis vectorises;
  mono runs the same kernel on lane 0. PolyBLEP residual in **f32** to dodge the
  x86 u32 mul-high. **Autovectorised, no new dependency, ASM-verified** in 0958
  (NEON + x86-64 AVX2); explicit SIMD only as a fallback if the ASM gate fails.

## Scope

**In:**

- `HyperSawCore` — pure, alloc-free voice-batched (16-wide, copy-major) 9-saw
  ensemble (`patches-dsp`): u32 phase, f32 PolyBLEP residual, fractional-density
  weighted sum, random phase init; autovectorised + ASM-verified.
- `HyperSaw` — mono module (`patches-modules`).
- `PolyHyperSaw` — 16-voice module (`patches-modules`).
- Module doc comments to the standard form; manual reference picks them up.

**Out (deferred):**

- Explicit SIMD (`std::simd`/`wide`). Only if 0958's autovec ASM gate fails on
  x86-64; ADR 0078 §7 / Open question 2.
- Hard-sync "synced ensemble" mode (ADR 0078 Open question 1).
- Poly spread/density/mix CV (mono/shared in v1).
- Phase modulation.

## Tickets

- [x] [0958 — HyperSawCore: fixed-point detuned saw ensemble (patches-dsp)](../../tickets/closed/0958-hypersaw-core.md)
- [x] [0959 — HyperSaw: mono module (patches-modules)](../../tickets/closed/0959-hypersaw-module-mono.md)
- [x] [0960 — PolyHyperSaw: 16-voice module (patches-modules)](../../tickets/closed/0960-hypersaw-module-poly.md)
- [x] [0961 — Docs + DSL corpus for HyperSaw/PolyHyperSaw](../../tickets/closed/0961-hypersaw-docs-corpus.md)

## Dependency order

```text
0958 (HyperSawCore) ──> 0959 (HyperSaw, mono) ──> 0960 (PolyHyperSaw) ──> 0961 (docs/corpus)
```

## Acceptance

- `HyperSaw` produces a detuned saw ensemble; spread `0` = unison single saw,
  spread `1` = ±50 cents outermost; density fades copies in symmetrically with
  no level jump; mix crossfades clean centre saw ↔ full stack.
- Spectrum shows PolyBLEP-suppressed aliasing; exactly one correction per wrap
  per copy (no double-emit/wrong-sign — cf. 0955/0956).
- Phases decorrelated at construction (no thin attack, no coincident-wrap comb).
- `PolyHyperSaw` does the above per-voice across 16 voices, `voct`/`fm` poly,
  spread/density/mix mono CV.
- FM behaves as control-rate vibrato.
- **Kernel vectorises**: 0958 benchmark recorded + ASM shows vector instructions
  in the hot loop on NEON *and* x86-64 AVX2 (else the explicit-SIMD fallback ran
  and is itself ASM-verified).
- `just commit` green for touched crates; `cargo clippy` clean.

## Open questions

1. **PolyBLEP polarity / quality.** Confirm sign against a reference spectrum in
   0958 (2-point form, f32 residual). 2-point is the target; revisit only if
   aliasing audibly intrudes under the detune/beating.
2. **Density boundary smoothness.** Confirm the single-fractional-pair sum
   optimisation matches a fully per-side-weighted sum to within rounding in
   0958 tests.
3. **x86 autovec.** The real risk. If 0958's AVX2 ASM gate shows a scalar hot
   loop after structuring, the explicit-SIMD fallback (`std::simd` nightly vs
   `wide` dep) is decided in 0958 with the disassembly as evidence (ADR 0078 §7).
