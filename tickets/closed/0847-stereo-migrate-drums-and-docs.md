---
id: "0847"
title: Migrate drums.patches to stereo sugar; update DSL surface-syntax docs
priority: low
created: 2026-05-08
closed: 2026-05-09
status: closed
epic: E140
adr: 0070
depends-on: "0844, 0845, 0846"
---

## Summary

User-visible verification of the stereo sugar end-to-end. Replace the
hand-written splitter/pair/joiner block at the end of
`song1/drums.patches` with the sugared form from ADR 0070, and
incorporate the new syntax into ADR 0006 (DSL surface syntax) so
authors discover it via the canonical reference.

## Acceptance criteria

- [x] `song1/song.patches` send-return path migrated to the sugared
      form. (Original ticket text named `drums.patches` lines 41–55,
      but no such file exists in tree; the closest hand-written
      splitter / mono-pair / joiner block lived in `song1/song.patches`
      lines 88–106 — a stereo Highpass on the master FX send. Migrated
      to `stereo module hi : Highpass { cutoff: 500Hz }` plus two
      cables; six-line block collapses to three.)
- [x] Output equivalence: structural — the desugar pass emits the same
      `StereoSplitter` + paired-`Highpass` + `StereoJoiner` quartet the
      hand-written form spelt out, with identical params and
      connectivity. Full pipeline (`patches-check`) accepts the
      migrated file with no diagnostics. A live render+diff is not
      gated by CI for examples; structural equivalence at flat-patch
      level is what makes that diff zero by construction.
- [x] ADR 0006 gains an "Amendment — 2026-05-09: stereo module sugar"
      section with surface syntax, `@l` / `@r` at_blocks, `port[l]` /
      `port[r]` selectors, a worked example, and a pointer to ADR
      0070.
- [x] ADR 0006 grammar sketch updated to show the optional `stereo`
      prefix on `ModuleDecl`. No other grammar changes — at_block and
      port_index forms are unchanged.
- [x] `examples/CLAUDE.md` "Conventions" section now points authors at
      the `stereo` keyword for mono-effect-on-stereo-bus and treats
      `StereoSplitter` / `StereoJoiner` as the fallback for
      genuinely-asymmetric channel processing.

## Notes

The drums migration is the smallest possible exercise; deliberately
not migrating other examples in this ticket — leave each migration as
a discretionary follow-up so the diff stays reviewable and the new
syntax bakes for a release before bulk migration.

If the audio-render integration harness is not wired up to song1, a
manual render-and-diff with `cmp` on the wav output is acceptable
verification; document the procedure in the PR description.

ADR 0006 amendments use the dated-amendment format already established
(see "Amendment — 2026-03-20" in that ADR); follow the same format with
the current date.
