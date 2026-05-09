---
id: "0846"
title: LSP intelligence for stereo module sugar
priority: medium
created: 2026-05-08
epic: E140
adr: 0070
depends-on: "0844, 0845"
---

## Summary

Surface stereo-module sugar through the LSP: hover, completion,
go-to-definition, inlay hints, and diagnostics. The LSP already
re-runs the desugar/expand pipeline per file
(`patches-lsp/src/expansion.rs`), so once tickets 0844 (desugar) and
0845 (tree-sitter parity) land, the LSP has both the syntax tree and
the expanded flat patch available. This ticket connects them to user-
facing features.

## Acceptance criteria

### Hover

- [ ] Hovering a `stereo module name : T` decl shows `T`'s descriptor
      with a `(stereo-paired)` annotation and a one-line description
      of the expansion (e.g. "expands to `name__l`, `name__r` with
      shared splitter/joiner").
- [ ] Hovering a port reference `crush.<port>[l]` or `crush.<port>[r]`
      resolves to the port descriptor on the underlying mono module
      type, identical to what a hover on `crush__l.<port>` would
      produce.
- [ ] Hovering the `[l]` / `[r]` token on a stereo-module port_ref
      shows "left side / right side of stereo module `<name>`" with
      a click-through to the decl.
- [ ] Hovering a bare `crush.<port>` (bus form) on a stereo module
      shows the port with a "(stereo bus — both sides)" annotation.

### Completion

- [ ] Typing `crush.` on a stereo module offers the module's port
      labels (the bus form). Side selectors come after the port label,
      not before, so completion at this position remains identical to
      a plain mono module.
- [ ] Typing `crush.<port>[` on a stereo module offers `l` and `r`
      as the only completions for that index position.
- [ ] Typing `module ` at statement scope offers `stereo` as a
      keyword completion alongside the module-decl pattern.
- [ ] Inside a `stereo module x : T { ... }` param block, completion
      offers `@l` and `@r` as at_block headers (alongside the existing
      param-name completions).

### Navigation

- [ ] Go-to-definition on a stereo module reference (any of `crush`,
      `crush.<port>`, `crush.<port>[l]`, `crush.<port>[r]`) lands at
      the `stereo module` decl in the source file.
- [ ] Find-references on a stereo module returns all references
      regardless of whether they use the bus or selector form.

### Inlay hints

- [ ] When the user-facing setting `patches.inlayStereoExpansion` is
      enabled, ghost-text inlay hints display the implicit splitter
      and joiner emissions at the `stereo module` decl site (e.g.
      `// + StereoSplitter __split_0, StereoJoiner __join_0`).
- [ ] Default off. The whole point of the sugar is to hide this
      detail; the inlay is for debugging unexpected expansions.

### Diagnostics

- [ ] Stereo source → mono port surfaces as a diagnostic at the cable
      site. The message names both endpoints and suggests the
      escape hatches from the ADR (`Sum` module, or `port[l]` /
      `port[r]`).
- [ ] Stereo source → `port[l]` / `port[r]` selector surfaces as a
      diagnostic at the cable site, suggesting picking a side from
      the source first.
- [ ] Wrapping a multi-channel module type with `stereo` (e.g.
      `stereo module x : Mixer(8)`) surfaces at the type-name token
      with a clear "stereo modules wrap a single-channel type" message.
- [ ] Identifier clash (user name ending in `__l` / `__r` colliding
      with a synthesised name) surfaces at the user's decl site, not
      at the synthesised one.
- [ ] All diagnostics carry source ranges that round-trip through
      `Url::to_file_path` cleanly on Windows (the existing LSP test
      harness covers this; re-use).

## Implementation notes

The LSP's AST-builder layer (`patches-lsp/src/ast_builder/module_decl.rs`)
needs an `is_stereo` flag matching the pest AST. The port_ref builder
needs no change — `[l]` / `[r]` selectors arrive through the existing
named-index path; the *interpretation* against a stereo module is the
expander's job.

The completion layer (`patches-lsp/src/completions/`) has separate
modules for module types, params, ports, shape, tap. Side-selector
completion fits into `ports.rs` (or wherever `port_index` completion
lives) with a stereo-module branch.

Hover and navigation share the resolved-symbol layer
(`patches-lsp/src/hover/`, `patches-lsp/src/navigation.rs`). The
side selector is a "virtual symbol" — it doesn't have its own decl,
so the navigation entry maps it back to the parent stereo decl.

Diagnostics flow from the desugar stage (ticket 0844). The LSP
forwards them through its existing diagnostic pipeline; this ticket
verifies that the end-to-end reporting reaches the editor with usable
ranges and messages.

## Out of scope

- Auto-import of `Sum` for the stereo→mono fix-it. The fix-it
  suggests it textually; actual auto-application can land later.
- Refactor "convert hand-written pair to `stereo`" code action.
  Tempting; defer to a follow-up so this ticket stays focused on
  surfacing the new syntax.
