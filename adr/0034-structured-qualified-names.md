# ADR 0034 — Structured qualified names

**Date:** 2026-04-14
**Status:** accepted

---

## Context

The DSL pipeline qualifies identifiers under a namespace (template instance,
song, etc.) by string concatenation. In `patches-dsl/src/expand.rs`:

```rust
fn qualify(namespace: Option<&str>, name: &str) -> String {
    match namespace {
        None => name.to_owned(),
        Some(ns) => format!("{}/{}", ns, name),
    }
}
```

Qualified names flow through the expander into module IDs, connection
endpoints, pattern bank keys, and song names in `FlatPatch`. Downstream
consumers (interpreter, LSP, SVG renderer) then split the string back apart
ad hoc — `patches-lsp/src/analysis.rs` defines `ScopeKey = (String, String)`
locally, and `patches-svg/src/lib.rs` uses `(String, String)` tuples for
port pairs.

Problems:

- Every consumer that needs the namespace re-parses the slash-separated
  string. Slicing is repeated and error-prone.
- The sigil (`/`) leaks into anywhere a qualified name is displayed, and
  changing it would touch every split-site.
- Planned features that deepen scoping — song-local patterns (ADR 0035),
  nested template expansions — make the flat string strategy increasingly
  awkward: each layer must pick a delimiter that cannot appear in any
  inner segment.

## Decision

Introduce a structured identifier type in `patches-core` (or a shared
location accessible to both `patches-dsl` and `patches-interpreter`):

```rust
pub struct QName {
    pub path: Vec<String>,   // outer → inner, empty for top-level
    pub name: String,
}

impl QName {
    pub fn bare(name: impl Into<String>) -> Self;
    pub fn child(&self, name: impl Into<String>) -> Self; // extends path
    pub fn is_bare(&self) -> bool;                         // path.is_empty()
}

impl fmt::Display for QName { /* joins with "/" */ }
```

`QName` replaces `String` anywhere the value semantically represents a
qualified identifier:

- Module IDs in `FlatPatch` (module names, connection endpoints).
- Pattern bank keys exposed by the expander.
- Song names in the expanded output.
- `NameScope` map values in the expander (currently `HashMap<String,
  String>` mapping local → qualified).

`qualify()` and `child_ns()` become methods on `QName`. `Display` is the
only place the sigil appears; all lookups compare by path + name without
string slicing.

## Alternatives considered

### Keep strings, formalise a helper module

Encapsulate `qualify` / `split_qname` in one crate and route all call
sites through it.

Rejected: does not fix the fundamental issue that the canonical
representation is a parsed form rather than a flat string. Every consumer
still pays a parse cost at use time.

### Intern qualified names as `Arc<str>`

Reduces allocation for repeated qualification but keeps the flat-string
ergonomics problem.

Rejected: orthogonal to the decision at hand. If profiling later shows
allocation pressure, `QName` can wrap `Arc<str>` segments without
affecting callers.

## Consequences

- Mechanical migration across `patches-dsl`, `patches-interpreter`,
  `patches-lsp`, `patches-svg`, and any code that stores module IDs.
- Serialisation surfaces (e.g. debug output, error messages, SVG node
  identifiers) use `Display`; existing tests that match on `"ns/name"`
  strings continue to pass.
- Unblocks ADR 0035 (song DSL overhaul): song-local patterns can be
  mangled by extending `path` rather than inventing a second-level sigil.
- Interpreter pattern-bank assignment (ADR 0029, alphabetical by name)
  becomes alphabetical by `Display` of `QName`, which is stable under
  the same constraints.
