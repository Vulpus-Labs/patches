# ADR 0036 — Source provenance through template expansion

**Date:** 2026-04-14
**Status:** accepted

---

## Context

The DSL pipeline is: parse → AST → `expand()` → FlatPatch → interpreter
→ engine. AST nodes and FlatPatch nodes both carry a single
`Span { start: usize, end: usize }` (byte offsets into a source string).
This is enough for direct syntactic diagnostics but loses information in
two ways:

1. **Template expansion discards the call chain.** In
   `patches-dsl/src/expand.rs`, `expand_template_instance` recursively
   walks a template body. The FlatModule / FlatConnection nodes it emits
   inherit spans from the template *definition*, not the call site. For
   nested calls (a template that calls another template) the intermediate
   sites are also lost. A failure deep inside a three-level expansion is
   reported against the innermost definition with no breadcrumb back to
   the author's code.

2. **`BuildError` has no origin.** Both `patches-core::BuildError`
   (module-level: `UnknownModule`, `InvalidShape`, `MissingParameter`,
   ...) and `patches-engine::builder::BuildError` (engine-level) are
   pure semantic descriptions. The interpreter knows which FlatModule
   triggered an error but cannot attach its span, so errors surfaced by
   `patches-player`, the CLAP host, and the LSP carry no source
   location.

3. **Spans have no file identity.** `Span` is raw byte offsets. The
   loader (ADR 0032) merges included files; after merge, a span's file
   of origin is no longer recoverable. Cross-file expansion chains
   cannot be rendered without a file identifier.

## Decision

Introduce source-provenance as a first-class concept across the DSL and
core error types.

### `SourceId` on `Span`

```rust
pub struct SourceId(pub u32);

pub struct Span {
    pub source: SourceId,
    pub start: usize,
    pub end: usize,
}
```

A `SourceMap` owns `(PathBuf, String)` per loaded file and assigns
monotonic IDs. The loader creates one `SourceMap` per patch load;
`SourceId(0)` is reserved for synthetic spans (nodes fabricated by
port-group expansion, shape-arg substitution, etc.).

### `Provenance` replaces single-span on Flat nodes

```rust
pub struct Provenance {
    pub site: Span,            // where the node "is" — innermost definition
    pub expansion: Vec<Span>,  // call sites, innermost first, outermost last
}
```

FlatModule, FlatConnection, FlatPortRef, FlatPatternDef, FlatSongRow,
FlatSongDef drop their `span` field in favour of `provenance`.

At each recursion into a template body, the expander clones the current
call chain, pushes the call site, and passes the extended chain down.
Nodes emitted in the body are tagged with that chain plus the
template-definition span as `site`.

### `origin: Option<Provenance>` on `BuildError`

Both BuildError enums gain an optional origin. The interpreter — which
holds the FlatModule being built — is the wrapping site: it attaches
provenance to errors returned by module constructors and validators.
`Option` keeps the door open for engine-internal errors (e.g.
`PoolExhausted`) that have no DSL origin.

### Rendering

- `patches-player`: resolve `SourceId → PathBuf` via the live
  `SourceMap`, print the site followed by one "expanded from
  <file:line:col>" note per expansion entry. Mirrors the shape the
  loader already uses for `LoadError::include_chain`.
- `patches-lsp`: map expansion entries to
  `DiagnosticRelatedInformation` — exactly the LSP primitive for this.
- `patches-clap`: same rendering path as the player.

## Consequences

### Positive

- Error messages for template-expanded patches become actionable.
  Authors see both the failing construct and the call path that created
  it.
- LSP gains expansion-aware diagnostics "for free" once the chain
  reaches `Diagnostic::related`.
- Unlocks future features that require traceable expansion: "peek
  expansion", hover showing the expanded module, go-to-call-site from
  flat nodes.
- `SourceId` decouples spans from merged source buffers and fixes a
  long-standing quiet bug in multi-file diagnostics.

### Negative

- Wide ripple: ~57 `BuildError::` call sites across ~9 files must be
  touched. Mechanical but tedious.
- Every `Span { start, end }` literal in tests and ast-builders must
  gain a `source` field. One-time churn.
- Clone cost at each expansion recursion (one `Vec<Span>` clone per
  template instance). Negligible at load time, zero at runtime.

### Neutral

- Loader's existing `include_chain` can migrate to `Provenance` or
  remain a parallel concept. We keep it separate initially; they solve
  adjacent problems (include resolution vs. template expansion).

## Alternatives considered

- **Keep single-span, render call chain from a separate side table.**
  Rejected: the side table would need to be keyed by a stable node
  identity, which FlatPatch does not currently provide, and every
  consumer would have to query it. Provenance on the node is simpler.
- **Interned `Arc<Provenance>`.** Premature; chains are short and
  template expansion is not hot. Revisit if load times regress.
- **Attach provenance only to errors, not to flat nodes.** Fails for
  downstream features (LSP hover, SVG tooltips) that need the chain at
  visualisation time, not just error time.

## Related

- ADR 0005 — DSL compilation pipeline
- ADR 0019 — Variable-arity templates
- ADR 0032 — Include directives (shares file-identity concerns)
