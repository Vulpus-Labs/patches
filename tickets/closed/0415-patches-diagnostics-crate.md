---
id: "0415"
title: Introduce patches-diagnostics crate (structured form)
priority: medium
created: 2026-04-14
epic: E076
depends_on: ["0414"]
---

## Summary

Create a new crate `patches-diagnostics` that exposes a presentation-
neutral structured form for author-facing diagnostics. It is consumed
by `patches-player` (terminal), `patches-clap` (GUI), and optionally
`patches-lsp`. It does no rendering itself — rendering lives in each
frontend.

## Proposed API

```rust
pub struct RenderedDiagnostic {
    pub severity: Severity,       // Error | Warning | Note
    pub code: Option<String>,     // e.g. "E-unknown-port"
    pub message: String,
    pub primary: Snippet,
    pub related: Vec<Snippet>,    // expansion chain, related decls
}

pub struct Snippet {
    pub source: SourceId,
    pub range: Range<usize>,      // byte offsets in source
    pub label: String,            // e.g. "unknown port", "expanded from here"
    pub kind: SnippetKind,        // Primary | Note | Expansion
}

pub enum Severity { Error, Warning, Note }
pub enum SnippetKind { Primary, Note, Expansion }
```

Plus a builder that converts `(BuildError, &SourceMap)` →
`RenderedDiagnostic`, pulling `origin.site` into `primary` and each
`origin.expansion` span into a `SnippetKind::Expansion` related entry.

## Acceptance criteria

- [ ] New crate `patches-diagnostics` in the workspace,
      `publish = false`.
- [ ] Depends only on `patches-core` (for `BuildError`) and
      `patches-dsl` (for `Span`, `SourceId`, `SourceMap`, `Provenance`).
      No rendering dependencies (no ariadne, no termcolor).
- [ ] `RenderedDiagnostic::from_build_error(&BuildError, &SourceMap)`
      constructor.
- [ ] Separate constructor for expand-time errors
      (`ExpandError` or whatever 0411 produces) so both error paths
      feed the same structure.
- [ ] Unit tests covering: error with no origin (primary is a
      synthetic "no source" snippet), error with 0/1/N expansion
      entries, cross-file expansion (different SourceIds).
- [ ] `cargo build`, `cargo test`, `cargo clippy` clean.

## Notes

- Keep it data-only. Any method on `RenderedDiagnostic` other than
  constructors and accessors is a smell — that logic belongs in the
  renderer.
- `Severity` could live in `patches-core` if it's useful elsewhere,
  but keeping it local to the diagnostics crate is fine for now.
