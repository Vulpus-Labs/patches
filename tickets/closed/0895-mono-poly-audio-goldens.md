---
id: "0895"
title: Audio integrity goldens for mono↔poly Audio conversion
priority: medium
created: 2026-05-15
epic: E147
adr: 0074
---

## Summary

Add audio-integrity golden corpus entries that exercise the two new
silent-conversion paths from ADR 0074: a patch where a mono LFO drives
all 16 poly voices (broadcast via synthetic `__autoconv_*` MonoToPoly),
and a patch where 16 poly voices sum to the mono output bus (sum-fold
via synthetic `__autoconv_*` PolyToMono). Both must produce
bit-identical output across runs.

## Acceptance criteria

- [ ] New golden patch: mono LFO/envelope → poly voice modulation
      relying on auto-conversion. Covers `mono Audio → poly Audio` via
      synthetic `MonoToPoly`.
- [ ] New golden patch: 16 poly voices summed into mono bus relying
      on auto-conversion. Covers `poly Audio → mono Audio` via
      synthetic `PolyToMono`.
- [ ] Companion patches exist where the user writes the `MonoToPoly` /
      `PolyToMono` explicitly. The auto-conversion and explicit patches
      produce **bit-identical** audio — proves the sugar is purely a
      desugaring with no semantic drift.
- [ ] Existing goldens unchanged — no patch in the corpus today relies
      on the rejection. Verify with `just push` that no existing
      golden hash drifts.
- [ ] `just smoke` green (full pipeline including golden corpus).

## Notes

Fits alongside existing audio-integrity goldens — check
`patches-integration-tests/` for the corpus location and naming
convention.

The bit-identity criterion is stronger here than for the Phase 2
fusion work (where timing-only differences were tolerated) — the
auto-conversion path inserts the same `MonoToPoly` / `PolyToMono`
module type the explicit path uses, with the same fusion treatment,
so equality is the expected outcome, not approximate match.
