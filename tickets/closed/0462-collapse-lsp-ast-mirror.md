---
id: "0462"
title: Collapse LSP tolerant AST mirror onto shared core
priority: high
created: 2026-04-15
status: closed-wontfix
---

## Resolution

Closed without action. On re-reading, the LSP AST is not a 1:1
mirror of the DSL AST: the drift maps in `ast.rs:380-493`
explicitly mark Song/Pattern statements, Pattern/Song param
types, RowGroup::Repeat, and the entire play/section composition
hierarchy as "not mirrored — outside LSP semantic model" or
"flattened in ast_builder". The LSP wants a deliberately
narrower, Option-wrapped subset.

A shared core AST therefore doesn't fit — it would force LSP to
carry variants it deliberately doesn't reason about. A
derive-macro approach (`#[derive(TolerantMirror)]` with
`#[tolerant(skip)]` annotations) would be the only honest
consolidation, and the payoff (kill ~400 LOC of mirror + drift
maps + parts of `ast_builder.rs` lowering) is not worth the
custom-derive infrastructure.

The existing `drift_maps_compile` test already eliminates the
silent-drift risk: any new DSL variant is a hard compile error
until triaged. The remaining cost is LOC, not coupling.

Revisit only if (a) the LSP semantic model expands to cover the
currently-skipped DSL constructs, or (b) the mirror demonstrably
becomes a maintenance bottleneck.

## Summary

`patches-lsp/src/ast.rs` (~400 LOC) hand-mirrors the DSL AST from
`patches-dsl/src/ast.rs`, wrapping fields in `Option` to tolerate
incomplete parse trees. The drift tests at the bottom of `ast.rs`
acknowledge the sync burden: any new variant in the DSL forces a
compile error in LSP until manually triaged.

Two sources of truth must be kept aligned forever. Each new DSL
construct doubles the work.

## Acceptance criteria

- [ ] One of:
      - shared core AST in `patches-core` (or `patches-dsl`) that
        both pest and tree-sitter lower into, with LSP applying
        `Option`-wrapping as a thin adapter; OR
      - generated mirror (macro or build-script) so the LSP AST
        derives from DSL AST automatically.
- [ ] Drift tests in `ast.rs` either deleted or repurposed as
      coverage for the adapter.
- [ ] No behaviour change in hover, completions, navigation,
      diagnostics.
- [ ] `cargo build`, `cargo test`, `cargo clippy` clean.

## Notes

E084. Highest maintenance tax in LSP. Tolerant-vs-strict split
is intentional; the *duplication* is not. Adapter layer is the
honest expression of the constraint.

Reference: existing intent comment in `ast.rs` calls the mirror
"intentionally independent" — that framing is what locked in
the drift problem.
