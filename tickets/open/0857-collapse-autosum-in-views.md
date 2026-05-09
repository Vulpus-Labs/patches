---
id: "0857"
title: Collapse __autosum_* nodes in graph views (SVG, LSP, profiler)
priority: low
created: 2026-05-09
related: "ticket 0852, ADR 0071 (rejected)"
---

## Summary

Ticket 0852's auto-Sum rewrite synthesises `__autosum_<target>_<port>`
nodes at descriptor-bind time so multi-fan-in patches type-check and
build. The synthesised nodes serve the engine but are noise in
user-facing graph views: the patch the user wrote and the graph the
view shows no longer match. ADR 0071 had proposed solving this
structurally with multi-source input ports; that ADR is rejected (see
its §Resolution). The remaining presentation gap lives at the view
layer and is what this ticket addresses.

Three surfaces consume the post-bind module graph and need to elide
or visually collapse autosum nodes:

1. **SVG export** (`patches-svg`). Today renders every `FlatModule`,
   autosum included, as a labelled rectangle with edges. Should render
   each autosum group as a small summing junction (e.g. a `+` on the
   target's input port) with the N source edges arriving directly at
   the consumer.
2. **LSP graph view / hover / inlay** (`patches-lsp`). Wherever the
   LSP exposes module-level structure (graph dumps, hover that names
   the producer of a connection, expansion-aware analysis), autosum
   nodes should report as the consumer's input port, not as a separate
   module.
3. **Profiler per-instance readouts** (`patches-profiling`). Per-module
   CPU bars surface autosum frames separately today. Either roll them
   into the consumer's bar with a sub-line, or hide them — pick the
   cheaper option once the structure is in front of you.

## Acceptance criteria

- [ ] Decide on the synthesised-node marker. Two viable shapes:
      (a) keep the name-prefix convention (`__autosum_*`) and have
      consumers test the prefix; (b) extend `FlatModule` (or its
      `Provenance`) with a `synthesised: SynthesisedKind` tag and
      have consumers match on it. (b) is the structurally correct
      one but costs a `patches-dsl` field; (a) is one-line per
      consumer. Pick one and document the choice in this ticket's
      notes before writing code.
- [ ] `patches-svg` collapses autosum groups: the synthesised module
      is not rendered as a node; instead its incoming edges land on
      the target port with a small `+` glyph (or equivalent layout
      token — see `patches-svg/src/render.rs` for the existing port-
      decoration vocabulary). Snapshot tests under
      `patches-svg/src/snapshots/` updated; new snapshot covering a
      multi-fan-in patch added.
- [ ] LSP analysis treats edges through autosum as edges to the
      target port. Hover on a fan-in cable names the user's producer
      module, not `__autosum_*`. Graph dumps (if any are user-facing)
      omit autosum entries. Workspace tests in
      `patches-lsp/src/workspace/tests/` extended to assert the
      transparency.
- [ ] Profiler decides between rolling autosum CPU into the consumer
      or hiding it. Implement and add a regression test covering a
      multi-fan-in patch (any existing profiling test exercising
      per-instance breakdown is the right neighbour).
- [ ] mdBook module reference (`docs/src/modules/`) carries no
      `Sum` / `PolySum` / `StereoSum` page changes — those modules
      are still present and user-callable; the view collapse is
      strictly for *synthesised* instances. Confirm during review.
- [ ] `just inner -p patches-svg -p patches-lsp -p patches-profiling
      -p patches-interpreter` green.

## Out of scope

- Retiring the `Sum` / `PolySum` / `StereoSum` modules. They stay
  (ADR 0071 rejected; the modules are what auto-Sum instantiates).
- Renaming `__autosum_*`. The convention is established and any
  consumer that wants to identify the synthesis can match the prefix
  if option (a) is chosen.
- The interaction with fusion (ADR 0072). Once cable delays inside
  acyclic SCCs are fused, autosum chains lose their 1-sample skew
  but keep their visual presence; the view collapse is independent
  of whether fusion has shipped.

## Notes

- Ticket 0852 is closed; its `fan_in.rs` rewrite generates the names
  this ticket consumes. See
  `patches-interpreter/src/descriptor_bind/fan_in.rs:242-261` for
  the `generate_sum_id` convention.
- `Provenance` already records the synthesis call site for autosum
  nodes (the bind pass tags them with the source span of the original
  multi-fan-in connection group). If option (b) lands, the new tag
  travels alongside `Provenance`, not replacing it — the span remains
  the right answer for "where did this synthesised node come from".
- Floating-point summation order is fixed by autosum's input order at
  bind. View collapse is presentation-only; no ordering or audio
  semantics change.
