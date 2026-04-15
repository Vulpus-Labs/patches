# ADR 0037 — Unified reference index for FlatPatch

**Date:** 2026-04-15
**Status:** accepted

---

## Context

`patches-lsp` drives feature handlers (hover, completions, goto, diagnostics)
off two parallel representations of the same file: the tolerant tree-sitter
AST (for syntax-broken files) and the pest `File` → `FlatPatch` (for clean
files, under ticket 0418). The expansion-aware hover added in ticket 0419
introduced several ad-hoc traversals of `FlatPatch`:

- `SpanIndex::find_at` — smallest enclosing authored span (existing).
- Call-site hover scans every `FlatModule.provenance.expansion` chain for
  the smallest enclosing span, then rescans modules to collect every node
  expanded under that call site.
- Connection-span hover rescans `flat.connections` to group fan-out
  siblings (all share one authored span but desugar to N flat entries).
- Template-wiring hover walks `merged.patch.body` plus every template
  body to locate the `ModuleDecl` enclosing the call site, then walks
  the matched template's body again once per declared port to collect
  `$.port → inner.port` wires.

For current patch sizes these scans are cheap (microseconds) and invoked
only on hover. The problem is not performance but **code shape**: every
feature reinvents the same traversal, each with slightly different cutoff
rules, and the expansion data model (per-node `Provenance` with a call
chain) is not easy to read backwards without a helper structure.

Three upcoming features will need the same information:

- **Inlay hints** — concrete poly widths and indexed-port ranges beside
  template calls. Needs "given this call site, enumerate emitted modules
  and their shapes" (already computed in hover, discarded).
- **Peek expansion** — code action rendering the full expanded body of
  a template call. Needs the reverse call-site index plus template body
  lookup.
- **Cross-cell diagnostics** — future signal-graph features (unused
  outputs, cycle warnings) need forward/reverse adjacency and
  call-site grouping.

Re-deriving these per feature multiplies complexity and drifts. The hover
code is already at the limit of how much ad-hoc traversal is readable.

## Decision

Introduce a **`PatchReferences`** structure in `patches-lsp::expansion`
that subsumes `SpanIndex` and precomputes the reverse and grouped views
that feature handlers need. Build once per root alongside `FlatPatch`
(in `ensure_flat`), cache with the same lifetime, invalidate together.

Scope:

```rust
pub(crate) struct PatchReferences {
    pub span_index: SpanIndex,

    /// Every span appearing in any node's expansion chain → flat nodes
    /// emitted under it. Used by call-site hover, inlay hints, peek.
    pub call_sites: HashMap<Span, Vec<FlatNodeRef>>,

    /// Authored connection span → indices of every FlatConnection with
    /// that span. Collapses top-level fan-out and arity expansion.
    pub connection_groups: HashMap<Span, Vec<usize>>,

    /// Instance QName → module index. Used to jump from port_ref to
    /// owning module without linear scan.
    pub module_by_qname: HashMap<QName, usize>,

    /// Call-site span → (template name, defining span). Precomputed
    /// from the merged pest `File` so hover does not re-walk bodies.
    pub template_by_call_site: HashMap<Span, TemplateRef>,

    /// Template name → precomputed wiring table (per declared port,
    /// the list of internal `$.port ↔ inner.port` connections from
    /// the template body).
    pub wires_by_template: HashMap<String, TemplateWires>,
}
```

`TemplateWires` flattens template-body `$` connections once at build
time so hover lookups are O(1). Fan-out and direction (forward/backward
arrows) are normalised during construction.

Constructor:

```rust
PatchReferences::build(flat: &FlatPatch, merged: &patches_dsl::File)
    -> PatchReferences
```

Replaces the existing `SpanIndex::build` call site in `ensure_flat_locked`.
`SpanIndex` stays as an inner field — no reason to collapse the two.

## Consequences

**Positive**

- One construction traversal per expansion, down from N-per-hover.
- Hover handlers become O(1) lookups rather than filters over the whole
  flat patch. Fan-out, template wiring, and call-site grouping are all
  table reads.
- Inlay hints, peek expansion, and future signal-graph features reuse
  the same structure without growing the hot surface.
- The expansion data model (provenance chain) is exposed through one
  interface rather than being re-inverted in each feature file.

**Negative**

- ~150 lines of builder code, plus tests. Memory cost: dominated by
  cloned `QName` and `Span` keys; empirically small for LSP-sized
  files (dozens to low hundreds of modules).
- Adds a layer between features and `FlatPatch` — new contributors must
  learn which index to consult rather than scanning directly. Mitigated
  by keeping the public API narrow and documenting the table purposes.

**Neutral**

- No changes to `patches-dsl` or `patches-core`. `PatchReferences` is
  LSP-private. If a similar structure proves useful to the SVG renderer
  or other consumers it can be lifted later; no need to pre-generalise.
- Cache invalidation already handled by `invalidate_flat_closure` — same
  lifetime as `flat_cache`, so no new coherence concerns.
