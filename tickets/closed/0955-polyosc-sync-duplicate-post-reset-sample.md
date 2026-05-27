---
id: "0955"
title: PolyOsc hard sync emits the post-reset sample twice (zero-order-hold glitch)
priority: medium
created: 2026-05-25
---

## Summary

`PolyOsc::process_voice_synced` resets the slave sub-sample and emits the
post-reset value **without advancing the phase**, while `process_voice_free`
emits-then-advances. Because the synced tick leaves `phase = (1-frac)·dt`, the
*next* free tick reads that same phase and emits the same value again before
advancing. The post-reset value lands on two consecutive samples — a one-sample
zero-order hold at every sync reset, injecting exactly the broadband HF that the
polyBLEP residual is there to suppress.

Root cause: a mismatched sampling convention between the two paths. Free ticks
emit the **start**-of-sample value (phase before advance); the synced tick emits
the **end**-of-sample value (`(1-frac)·dt`, the phase at the next sample
boundary). Those two coincide, so sample *n* (synced) and sample *n+1* (free)
output the same number.

Surfaced while porting this path to vxn-1 (`vxn-dsp::poly::process_pair`,
ticket 0020). There the slave keeps a single start-of-sample convention and
**defers** the band-limited post value one sample (sample *n* emits the pre
value, *n+1* emits the post value), so no duplicate occurs and the measured
synced-saw aliasing is 1.58×–2.01× below an un-BLEP'd reset across four ratios.

## Reproduction / trace

Slave saw, `dt = 0.1`, sync at sample *n* with `frac = 0.5`:

- sample *n* (synced): `sync_reset` → `phase = (1-0.5)·0.1 = 0.05`, emit
  `value(0.05)`.
- sample *n+1* (free): reads `phase = 0.05`, emits `value(0.05)` **again**, then
  advances to `0.15`.

`value(0.05)` appears on both samples.

## Why the existing test misses it

`patches-integration-tests/tests/hard_sync_aliasing.rs` compares the typed
sub-sample chain against a sample-**rounded** (via-pulse) chain. Both carry the
duplicate sample, so it cancels out of the high-band-energy ratio (margin only
~1.2×). The artifact only shows against a *clean* reference.

## Acceptance criteria

- [x] Confirm (or refute) the duplicate empirically: a unit test that hard-syncs
      a slave saw and asserts no two consecutive output samples are bit-equal
      across a reset where `frac ∉ {≈0, ≈1}`.
      → `sync_does_not_duplicate_post_reset_sample` (poly_osc.rs). Confirmed:
      pre-fix sine/triangle duplicated the post value exactly.
- [x] If confirmed, fix the convention mismatch so the post-reset value is
      emitted once (e.g. defer the post value one sample, matching the free
      path's start-of-sample convention, as vxn-dsp does).
      → Deferred start-of-sample convention + 2-point polyBLEP (see resolution).
- [x] Add an aliasing assertion against a **high-oversampled clean reference**
      (not the rounded path) so the regression is caught going forward.
      → `sync_aliasing_below_clean_reset_reference`: BLEP path aliases ≥1.4×
      below the sub-sample-accurate un-BLEP'd reset (vxn observes 1.58–2.01×).
- [x] `poly_sync_is_per_voice` and the existing `hard_sync_aliasing` ratios
      still hold. → both green; unsynced voices are bit-identical to pre-fix.

## Resolution

`PolyOsc::process_voice_synced` now follows the free path's **start-of-sample**
convention: the sync tick emits the value the voice already holds (the *pre*
value), resets the phase, and **defers the post value to the next tick** — so
the post value lands on exactly one sample. The phase reset replaces the
advance, so no duplicate.

The single after-only residual was replaced with a **2-point polyBLEP**: the
leading half is applied on the sync tick (`before = polyblep(1 − frac·dt, dt)`),
the trailing half is stashed in per-voice `pending_*_blep` and applied on the
post tick in place of the natural wrap correction (`after = polyblep(post_raw,
dt)`). Correction sign is `−basis · 0.5 · delta`, matching the free path's
`value − polyblep(…)` convention — the old `sync_blep_residual` carried the
opposite sign, which the relative `hard_sync_aliasing` test could not catch
(both chains shared it). The new clean-reference test pins the sign down.

`pending` is consumed by both the free path and `process_voice_synced` (the
rapid per-tick-sync case), so the deferred residual is never dropped. Unsynced
voices stay on the unchanged free path → bit-identical output.

### Follow-ups (same bug, separate code — out of scope here)

- Mono `Oscillator::process` (oscillator.rs:228–259) has the identical
  emit-post-without-advance convention and the same single-point residual sign.
- `patches-vintage` (sibling repo) `VDco`/`VPolyDco` hard-sync BLEP path
  (`render_sync_and_advance`, core.rs:268) duplicates likewise; soft-sync path
  is clean. Needs its own ticket in that repo.

## Notes

- DSP-only; per-voice, branchless/vectorised path must stay so. No param/UI
  change.
- Cross-ref: vxn-1 ticket 0020 (`vxn-dsp::poly::process_pair`) implements the
  duplicate-free variant — the 2-point polyBLEP with a deferred post sample.
- Audibility unverified — this ticket is the investigation. The glitch is one
  sample per sync event, so it may be subtle at audio rates but defeats the
  point of the sub-sample work at high sync ratios.
