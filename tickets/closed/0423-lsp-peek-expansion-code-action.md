---
id: "0423"
title: LSP peek-expansion code action for template calls
priority: medium
created: 2026-04-15
---

## Summary

Add a code action available at template call sites that renders the
expanded body of the call: the concrete modules and connections emitted
under that call, with shapes substituted. Uses `PatchReferences` to
enumerate emitted nodes; renders from the flat view rather than the
source template so the user sees what was actually produced.

See ADR 0037 and epic E078.

## Acceptance criteria

- [ ] Code action registered under a dedicated kind (e.g.
      `source.peekExpansion`) in addition to the existing `QUICKFIX`
      handling (`patches-lsp/src/main.rs:159-199`).
- [ ] Triggered when the cursor falls inside a span recorded in
      `PatchReferences::template_by_call_site`.
- [ ] Action returns a virtual document (or markdown payload via a
      custom command) listing the modules and connections emitted by
      the call. Modules show `name : type { shape }`; connections show
      `from -> to` using their concrete `QName`s.
- [ ] Implementation traverses `PatchReferences::call_sites` to find
      the emitted `FlatNodeRef`s, then resolves connections by walking
      `flat.connections` filtered to those module qnames. No new index
      field required for this ticket if the connection filter is
      acceptable; if it proves slow, add an `(call_site_span ->
      Vec<FlatConnRef>)` table to `PatchReferences` in a follow-up.
- [ ] Tests cover: simple template call, nested template call (peek
      shows the outer expansion only — inner template calls are still
      rendered as their final expanded modules, since the flat view is
      already fully expanded), call with fan-out.
- [ ] `cargo build`, `cargo test -p patches-lsp`, `cargo clippy` clean.

## Notes

Decision recorded for E078: render from flat view, not from source
template AST. Reasons:

- Reuses existing `PatchReferences` data with no AST printer needed.
- Shows substituted shapes — closer to what the engine will actually
  build.
- Avoids the question of how to render nested template calls within
  the peeked body; the flat view answers it (fully expanded).

Trade-off: the user does not see the template's source-level structure
(loops, conditionals). If that becomes a wanted feature, a separate
"show template source" action can sit alongside this one.
