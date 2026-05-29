---
id: "0973"
title: Regenerate + audition audio goldens for Q32-migrated modules
priority: medium
created: 2026-05-29
---

## Summary

The LFO and Op migrations (0971, 0972) change the phase representation, so any
audio golden or feedback patch exercising `Lfo` / `PolyLfo` / `Op` / `PolyOp`
shifts its sample values. Audition the affected outputs, confirm the change is
the expected precision/representation shift (and, for LFO, the low-rate fix —
not a regression), then regenerate the goldens.

## Acceptance criteria

- [x] Identify every golden / fixture / feedback patch touching the four
      migrated modules (grep the golden harness inputs + `examples/`).
- [x] Audition each: the change is the expected Q32 shift, not a defect; for slow
      LFOs confirm the cycle no longer stalls.
- [x] Regenerate the affected goldens; diff is bounded to the migrated modules.
- [x] PR notes which feedback patches changed and why (precision/representation),
      with a one-line audition note per non-trivial case.
- [x] `just push` green (full pipeline, since goldens are cross-cutting).

## Resolution

**No stored audio sample goldens exist.** Audio fidelity in this tree is pinned
by *behavioural* integration tests, not byte-exact sample dumps, so there is
nothing to regenerate:

- `auto_conv_audio_integrity.rs` self-compares two patches that *both* use the
  same `Lfo`, so the Q32 shift cancels — stays bit-identical, no change.
- `hard_sync_aliasing.rs`'s "golden" is `Oscillator`/`PolyOsc` (NOT migrated).
- No `.wav` / `include_bytes!` / `insta` audio snapshots anywhere (the only
  `insta` goldens are graph-JSON, tickets 0963/0964 — out of scope).

**Audition (throwaway headless render of all 14 example patches using the four
modules, 1 s at 48 kHz):**

- All 14 finite and bounded under Q32 — no NaN/Inf, no blow-up.
- `square_440.patches` (self-driving `Lfo`): peak 0.872, RMS 0.763 — correct.
- The other 13 are MIDI-driven synths (silent headless without a note); their
  `Op`/`PolyOp`/`Lfo` waveform-shape, phase-reset, voct-independence and
  slow-LFO-no-stall behaviour is pinned by the module unit tests (all green) and
  the PolyOp FM perf delta by `osc_fixedpoint_bench op` (70.3% of f32).

## Notes

- ADR 0080 §4, Epic E159. Depends on 0971 and 0972.
- No feedback patch on these modules is required to stay bit-identical (contrast
  the fusion Phase 2 constraint, where feedback patches had to). The shift here
  is expected and accepted.
- Pre-existing, unrelated: rendering some MIDI-synth examples headlessly via the
  default planner trips an ADR 0072 phase-5 `cable_pool` scratch-slot invariant
  (`scratch slot read with fused=false`). The engine catches it and halts cleanly
  (ADR 0051); `alloc_trap.rs` already renders one of these the same way. It is a
  planner/topology issue independent of phase representation — flagged, not fixed
  here.
- `just push` also required dedenting an over-indented doc list in
  `osc_fixedpoint_bench.rs` (clippy 1.95 `doc_overindented_list_items`, toolchain
  drift — pre-existing in the untracked bench, not caused by the migration).
- If a golden's change looks like more than a representation shift, stop and
  re-open the relevant migration ticket — do not paper over a regression by
  regenerating.
