---
id: "0420"
title: Introduce PatchReferences index and builder
priority: medium
created: 2026-04-15
---

## Summary

Replace the bare `SpanIndex` cached per root with a `PatchReferences`
structure that also precomputes call-site grouping, fan-out grouping,
qname→module lookup, template-by-call-site, and per-template wiring
tables. Build once in `ensure_flat_locked` from `(FlatPatch, &File)`;
invalidate with `flat_cache`. Feature handler migration lands in ticket
0421 — this ticket ships only the data structure, builder, and tests.

See ADR 0037.

## Acceptance criteria

- [ ] `patches-lsp::expansion` exposes `PatchReferences` with fields:
      `span_index`, `call_sites`, `connection_groups`, `module_by_qname`,
      `template_by_call_site`, `wires_by_template`.
- [ ] `PatchReferences::build(&FlatPatch, &patches_dsl::File)` populates
      all tables in a single pass per source (modules, connections,
      port_refs for inverse tables; merged file's patch body + templates
      for call-site → template; template bodies for wires).
- [ ] `WorkspaceState.span_index` replaced by `references: HashMap<Url,
      PatchReferences>`. `ensure_flat_locked` builds and stores it.
      `invalidate_flat_closure` and `prune_flat_caches` drop the new
      map alongside the others.
- [ ] `TemplateWires` structure records, per declared in/out port, the
      internal module ports wired to `$.port`. Fan-out collects all
      targets; backward arrows are normalised to forward semantics so
      consumers do not re-interpret `Direction`.
- [ ] `call_sites` keys cover every span appearing in any `Provenance.
      expansion` chain (not only the outermost); values collect
      `FlatNodeRef`s for modules, connections, port_refs, patterns, and
      songs emitted under that span.
- [ ] `connection_groups` keys are the authored spans of `FlatConnection.
      provenance.site`; values are indices into `flat.connections` so
      consumers read without re-filtering.
- [ ] Unit tests:
      - Call-site grouping: two template calls of the same template
        produce disjoint call-site entries; each contains every emitted
        module from that call.
      - Fan-out grouping: `a.out -> b.in, c.in` desugars to 2 connection
        entries under one group key.
      - Template wires: forward and backward `$.port` connections
        surface with the correct internal port list; multi-target
        fan-out lists every target.
      - `module_by_qname` round-trips nested template instance names
        (`v/osc`, `v/mix`).
- [ ] `cargo clippy` and `cargo test` clean for `patches-lsp`.

## Notes

Builder shape (single pass per source):

```rust
impl PatchReferences {
    pub(crate) fn build(flat: &FlatPatch, merged: &File) -> Self {
        let span_index = SpanIndex::build(flat);
        let mut call_sites = HashMap::new();
        let mut connection_groups = HashMap::new();
        let mut module_by_qname = HashMap::new();
        // ... populate from flat.modules / connections / port_refs

        let template_by_call_site = index_template_calls(merged);
        let wires_by_template = merged
            .templates
            .iter()
            .map(|t| (t.name.name.clone(), TemplateWires::from_body(&t.body)))
            .collect();

        Self { span_index, call_sites, connection_groups,
               module_by_qname, template_by_call_site, wires_by_template }
    }
}
```

Existing `SpanIndex` stays as an inner field — no reason to merge the
two; `find_at` is still useful and orthogonal to inverse lookups.

No feature code changes here. `compute_expansion_hover` continues to
receive `&SpanIndex` at its old call site; the migration happens in
ticket 0421.
