---
id: "0903"
title: Manual — DSL chapters
priority: medium
created: 2026-05-17
epic: E149
---

## Summary

Write the four new DSL chapters (basics, signal kinds & cable
rules, templates & abstraction, poly & voice allocation) and audit
the existing `dsl-reference.md` for consistency with the new
framing.

## Acceptance criteria

- [ ] `docs/src/dsl-basics.md` — `patch { ... }` block, `module name
      : Type` declaration, parameter braces, literal forms (float,
      int, frequency `440Hz`, decibels `-6dB`, note names `C4`,
      booleans), shape arguments `(channels: N)`, basic connection
      `a.out -> b.in`, scaled connection `-[0.5]->`, indexed ports
      `mix.in[0]`, at-blocks for grouped indexed params. Harvest
      from deleted `docs/src/building-a-patch.md` (in git history)
      and rewrite for the new audience.
- [ ] `docs/src/dsl-signal-kinds.md` — recap the five kinds from
      mental-model, document the cable-kind rules (mono→stereo
      broadcast, stereo→mono rejected, mono↔poly via MonoToPoly /
      PolyToMono, fan-out free, fan-in via mixer). Reference
      ADR 0059 for the symmetric-stereo port convention.
- [ ] `docs/src/dsl-templates.md` — template definitions, template
      parameters, instantiation, the typed-facade-over-dynamic-
      instantiation model. Verify against current parser /
      interpreter behaviour before writing; memory note
      `project_dsl_type_split` may be stale.
- [ ] `docs/src/dsl-poly.md` — channel count (typically 16),
      `PolyMidiToCv` voice allocation (LIFO voice stealing
      semantics), MonoToPoly broadcast, PolyToMono summation. If
      typed poly ports (memory: `project_typed_poly_ports`) have
      landed, document; if not, skip.
- [ ] `docs/src/dsl-reference.md` — audit existing file. Reorganise
      if needed to be a true reference (grammar, every syntax form,
      no narrative). Narrative belongs in the four chapters above;
      reference is for lookup.

## Notes

- Deleted source: `git show <commit>:docs/src/building-a-patch.md`
  before the delete commit lands; afterwards walk history. Most of
  dsl-basics harvests directly.
- Pest grammar source of truth: `patches-dsl/src/grammar.pest`.
- Templates: `patches-dsl/` has the expander; check current syntax.
- Cross-reference Mental model chapter rather than re-explaining
  signal-kind concepts.
- Cable-rule details from ADRs 0059 (port naming) and 0072
  (subgraph fusion).
