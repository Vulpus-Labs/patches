---
id: "E078"
title: LSP expansion UX and signal graph
created: 2026-04-15
closed: 2026-04-15
status: closed
depends_on: ["E077", "ADR-0038"]
tickets: ["0422", "0423", "0425"]
---

## Status

Closed 2026-04-15. ADR 0038 landed (unified patch loading pipeline), which
unblocked the tickets. `ExpandError` now surfaces through the staged
pipeline's diagnostic path (0425). Inlay hints register an
`inlay_hint_provider` capability and render concrete shapes + indexed-port
ranges via a shared `shape_render` module consumed by hover too (0422).
Template call sites carry a `source.peekExpansion` code action whose
command payload is the rendered expansion body (0423).

Ticket 0424 (signal graph + `SG0001` unused-output warnings) was reverted
2026-04-16: the warning produced too much noise during live-coding and was
not worth the upkeep. `SignalGraph` and its diagnostics were deleted.

## Summary

E077 landed `PatchReferences` (ADR 0037) as the canonical per-root index
for expansion-aware LSP queries, and migrated hover onto it. The index
was built deliberately without writing the consumers that motivate it.
This epic adds those consumers:

- **Inlay hints** — concrete poly widths and indexed-port ranges beside
  template calls. Reuses `call_sites` + `template_by_call_site`; needs
  shape-evaluation helpers lifted out of `analysis.rs` so hover and
  inlay hints share a single shape renderer.
- **Peek expansion** — code action that renders the expanded body of a
  template call. Reuses `call_sites` reverse mapping; renders from the
  flat view (post-substitution) rather than the source template, so the
  user sees what was actually emitted with concrete shapes.
- **Cross-cell signal graph** — adjacency index keyed by
  `(QName, PortLabel)` to support unused-output diagnostics. Signal-level
  cycles are legal at every granularity (1-sample cable delay) so no
  signal cycle detection is needed.
- **Surface expansion errors** — the DSL expander already detects
  structural errors (including recursive template instantiation) but
  the LSP silently swallows them. Publish them as diagnostics.

Each ticket extends `PatchReferences` only where the existing tables are
insufficient; the signal graph is a new sibling structure rather than an
extension, since its size and rebuild cost are different.

Out of scope:

- Cross-crate exposure of the signal graph.
- Completions changes (separate epic).

## Acceptance criteria

- [ ] Inlay hints show concrete shape and indexed-port ranges at template
      call sites; LSP `inlay_hint_provider` capability registered.
- [ ] Peek-expansion code action available at template call sites,
      returning the post-substitution flat body for that call.
- [ ] `ExpandError` from the DSL expander surfaces as an LSP diagnostic
      instead of being silently dropped.
- [ ] Shape evaluation helpers live in one module, consumed by hover
      and inlay hints; no duplicated walks of `FlatModule.shape`.
- [ ] `cargo build`, `cargo test`, `cargo clippy` clean.

## Tickets

| ID   | Title                                                |
|------|------------------------------------------------------|
| 0422 | LSP inlay hints for poly widths and indexed ports    |
| 0423 | LSP peek-expansion code action for template calls    |
| 0425 | Surface DSL expansion errors as LSP diagnostics      |
