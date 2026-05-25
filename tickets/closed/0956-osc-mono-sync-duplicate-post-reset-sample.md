---
id: "0956"
title: Mono Oscillator hard sync emits the post-reset sample twice (same as 0955)
priority: medium
created: 2026-05-25
---

## Summary

Mono `Oscillator::process` has the identical hard-sync defect fixed for
`PolyOsc` in ticket 0955. On a sub-sample sync the synced branch resets the
phase to `(1 - frac)·dt`, emits the **post** value, and **does not advance**
(`if sync.is_none() { wrap_frac = self.phase_acc.advance_wrap_frac(); }`). The
next free tick re-reads that same phase and emits the post value again — a
one-sample zero-order hold (an exact duplicate on sine/triangle), injecting the
broadband HF the polyBLEP residual exists to suppress.

It also shares 0955's second defect: the single after-only `sync_blep_residual`
carries the **wrong sign** relative to the free path's `value − polyblep(…)`
convention. The relative `hard_sync_aliasing` integration test cannot catch this
because both the direct and via-pulse chains share the residual.

Location: `patches-modules/src/osc/oscillator.rs:228–259` (synced branch) and
`:309–311` (the advance guard).

## Acceptance criteria

- [x] Port the 0955 fix: deferred start-of-sample convention (sync tick emits
      the *pre* value, defers *post* one sample), 2-point polyBLEP (leading half
      on the sync tick, trailing half applied on the post tick in place of the
      natural wrap correction), with the `−basis · 0.5 · delta` sign.
- [x] Unit test: no two consecutive output samples are bit-equal across a reset
      where `frac ∉ {≈0, ≈1}` (`sync_does_not_duplicate_post_reset_sample`).
- [x] Aliasing assertion against an un-BLEP'd clean reset reference
      (`sync_aliasing_below_clean_reset_reference`, ≥1.4× below).
- [x] `sync_resets_saw_to_post_advance`, `sync_all_waveforms_finite` still hold;
      `hard_sync_aliasing` integration re-tuned (see resolution).

## Resolution

Ported the 0955 fix to `Oscillator::process` (deferred start-of-sample
convention + 2-point polyBLEP with per-instance `pending_*_blep`). Unsynced
output is bit-identical to pre-fix. `sync_blep_residual` was now dead in-tree
and carried the wrong sign, so it was **removed** from `patches-dsp` (fn +
re-export); `polyblep`'s doc now describes the 2-point recipe its callers build.

`hard_sync_aliasing` (mono `Oscillator`) margins moved as anticipated: the
non-integer ratios **widened** to 1.76×–2.01× (were ~1.6×), the degenerate 2:1
case **tightened** to 1.11× (was ~1.3× — slave phase lands ≈0 at every sync, so
sub-sample frac barely matters). Direct still beats via-pulse at every ratio;
`ALIAS_MARGIN` re-tuned 1.2 → 1.08 with an updated comment.

## Notes

- DSP-only; mono path, no param/UI change.
- Cross-ref: ticket 0955 (PolyOsc, closed) — the resolution there is the
  template. Consider whether `sync_blep_residual` (shared with PolyOsc pre-fix)
  should be retired or sign-corrected in `patches-dsp` once both callers are
  fixed; PolyOsc no longer uses it.
- Sibling: VDco/VPolyDco in the `patches-bundles` repo carry the same bug
  (tracked there).
