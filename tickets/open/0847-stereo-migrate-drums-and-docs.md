---
id: "0847"
title: Migrate drums.patches to stereo sugar; update DSL surface-syntax docs
priority: low
created: 2026-05-08
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

- [ ] `song1/drums.patches` lines 41–55 replaced with the sugared form:
      `stereo module out_crush : Bitcrusher { depth: 8, rate: 0.8 }`
      and the three connections shown in ADR 0070 §"Worked example".
- [ ] Rendered audio from the sugared `drums.patches` is sample-equal
      (within float tolerance) to a render from the pre-migration
      file. Check via the existing audio-render integration harness
      or an ad-hoc render-and-diff if no such harness covers song1.
- [ ] ADR 0006 gains a new top-level section after "Module declarations"
      titled "Stereo module sugar" with:
  - the `stereo` keyword syntax
  - per-channel `@l: { ... }` / `@r: { ... }` at_blocks inside the
    regular param block
  - `port[l]` / `port[r]` channel selectors
  - one worked example
  - a pointer to ADR 0070 for full expansion rules
- [ ] ADR 0006 grammar sketch updated to reflect the optional `stereo`
      prefix on `module_decl` (no other grammar changes needed —
      at_block and port_index forms are unchanged).
- [ ] Any user-facing tutorial / README that introduces stereo
      processing in patch authoring points at the sugar form first
      and the hand-written form as the fallback.

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
