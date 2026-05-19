---
id: "0934"
title: Manual + module-reference docs update for E151
priority: medium
created: 2026-05-18
epic: E151
---

## Summary

Every new module from E151 needs a manual page under
`docs/src/modules/` matching the table form mandated by `CLAUDE.md`
(brief, extended description, inputs / outputs / parameters tables,
technical notes). Port names in the docs must match the strings in
each module's `ModuleDescriptor`.

The source-tree reorg is invisible to docs — the manual organises
by concept, not by directory.

## Scope

- `docs/src/modules/compressor.md`, `stereo_compressor.md`
- `docs/src/modules/gate.md`, `stereo_gate.md`
- `docs/src/modules/audio_to_trigger.md` (+ stereo / poly variants;
  a single page covering all three is fine if the variant table is
  clear)
- `docs/src/modules/audio_to_gate.md` (same)
- `docs/src/modules/pan.md`, `balance.md`, `stereo_width.md`,
  `mid_side.md`, `mono_bass.md`
- `docs/src/modules/dc_blocker.md`, `comb.md`
- Update `docs/src/SUMMARY.md` (or equivalent index) to include the
  new pages.
- Cross-reference ADR 0076 from the dynamics and detector pages
  (sidechain convention, linked-detector rationale,
  hysteresis-controls-eligibility note).

## Acceptance criteria

- [ ] One manual page per new module (or one page covering a
      mono/stereo/poly family, with a clear variant table).
- [ ] Port names in the docs match each module's
      `ModuleDescriptor` strings.
- [ ] `docs/src/SUMMARY.md` updated.
- [ ] mdBook builds clean (`just build-docs` or equivalent).
- [ ] Tables pass `tools/align-tables.py` if the alignment tool is
      part of the docs lint.

## Notes

The doc-comment-as-source-of-truth rule from `CLAUDE.md` applies:
the manual page mirrors the module's doc comment. If they diverge,
the doc comment wins and this ticket updates the manual.
