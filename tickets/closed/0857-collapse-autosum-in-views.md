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

- [x] Decide on the synthesised-node marker. **Chose (a) — name
      prefix.** See Notes for rationale; consumers go through the
      shared `QName::is_autosum()` / `is_autosum_name(&str)` helpers
      in `patches-core::qname`.
- [x] `patches-svg` collapses autosum groups: the synthesised module
      is not rendered as a node; instead its incoming edges land on
      the target port with a small `+` glyph. New snapshot
      `patches_svg__tests__autosum_collapse.snap` covers a
      two-source fan-in; existing snapshots updated for the added
      `.input-sum` CSS rule. Implementation in
      `patches-svg/src/flat_to_layout.rs` (collapse + edge rewrite +
      `EdgeOrigin` plumbing) and `patches-svg/src/render.rs`
      (`+` glyph emission).
- [x] LSP analysis treats edges through autosum as edges to the
      target port. The LSP's user-facing paths walk the *pre-bind*
      `FlatPatch`, so `__autosum_*` was never exposed to begin with;
      a regression test in
      `patches-lsp/src/workspace/tests/spans.rs`
      (`fan_in_hover_does_not_expose_autosum_synthetic_name`) pins
      the invariant. There are no user-facing graph dumps.
- [x] Profiler entries cannot carry the `__autosum_*` synthesised
      name — the collector keys on the descriptor's *type* name and
      a numeric `InstanceId`. Regression test
      `autosum_synthesised_names_do_not_reach_collector` in
      `patches-profiling/src/timing_collector.rs` pins the
      invariant. (See Notes for why "rolling into the consumer" is
      out of scope.)
- [x] mdBook module reference (`docs/src/modules/`) unchanged —
      `Sum` / `PolySum` / `StereoSum` pages still describe the
      user-callable modules; view collapse only suppresses
      synthesised instances.
- [x] `just inner -p patches-svg -p patches-lsp -p patches-profiling
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

- **Marker choice: option (a) — name-prefix.** A single
  `AUTOSUM_PREFIX = "__autosum_"` constant + a `QName::is_autosum()`
  helper now lives in `patches-core::qname`, alongside the
  free-function `is_autosum_name(&str)` for callers that only have
  a string in hand. Rationale: option (b) would have grown a
  `synthesised: SynthesisedKind` field on `FlatModule` (or
  `Provenance`) that has no semantic role outside the bind pass
  and would propagate through serialisation, FFI, and every
  downstream type that mirrors `FlatModule`. The prefix convention
  is already load-bearing in `patches-interpreter` (the bind pass
  generates it; tests assert it; error-collection branches read
  it), so option (a) just hoists what already exists into a shared
  helper.
- **Profiler finding.** The acceptance criterion's premise that the
  profiler "surfaces autosum frames separately today" was incorrect.
  The profiler keys its collector by `(InstanceId, &'static str)`
  where the string is `Module::descriptor().module_name` — the
  *type* name (e.g. `"Sum"`), not the QName instance id. Autosum
  CPU is therefore already aggregated into the `Sum` / `PolySum` /
  `StereoSum` type bars; the synthesised QName never reaches the
  profiler. A regression test in
  `patches-profiling/src/timing_collector.rs` pins the invariant.
  Distinguishing autosum CPU from user-authored `Sum` instances at
  the per-instance level would require plumbing a QName↔InstanceId
  map into the profiler — out of scope here.
- **LSP finding.** No `__autosum_*` exposure existed in the LSP
  before this ticket: the inlay-hint and hover paths walk the
  *pre-bind* `FlatPatch`, and `BoundPatch` is only used to look up
  bound descriptors via `find_module(&user_id)` — never iterated
  to surface synthesised names. A workspace test in
  `patches-lsp/src/workspace/tests/spans.rs`
  (`fan_in_hover_does_not_expose_autosum_synthetic_name`) guards
  against any future change that routes a user-facing surface
  through `BoundPatch.modules`.
- Ticket 0852 is closed; its `fan_in.rs` rewrite generates the names
  this ticket consumes. See
  `patches-interpreter/src/descriptor_bind/fan_in.rs:242-261` for
  the `generate_sum_id` convention (now using `AUTOSUM_PREFIX`).
- `Provenance` already records the synthesis call site for autosum
  nodes (the bind pass tags them with the source span of the original
  multi-fan-in connection group). If option (b) lands, the new tag
  travels alongside `Provenance`, not replacing it — the span remains
  the right answer for "where did this synthesised node come from".
- Floating-point summation order is fixed by autosum's input order at
  bind. View collapse is presentation-only; no ordering or audio
  semantics change.
