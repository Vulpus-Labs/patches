---
id: "0419"
title: LSP expansion-aware hover
priority: medium
created: 2026-04-14
---

## Summary

Extend the LSP hover handler to use the cached `FlatPatch` and span→FlatNode reverse index from ticket 0418. When the cursor is over a template use or inside a template body, show information derived from the expanded form: concrete module descriptors, resolved poly port counts (e.g. `in/0, in/1` when `channels=2`), and resolved parameter values. Fall back to the existing tolerant-AST hover when expansion is unavailable (syntax-broken file, missing include, etc.).

## Acceptance criteria

- [ ] Hover handler consults `flat_cache` + `span_index` before falling back to `SemanticModel`
- [ ] Hover over a template *use* shows the expanded module list (concrete module types and counts) instead of only the template signature
- [ ] Hover over a module instance inside a template *body* shows the resolved descriptor for the enclosing expansion context — concrete channel count, indexed port range (`port[i]`, i in 0..N−1)
- [ ] Hover over a port reference shows the fully-expanded port name where applicable (`in/0` rather than `in[i]`)
- [ ] Hover over parameter arguments shows the resolved value after template substitution
- [ ] Broken-syntax or expansion-failed file: hover falls back silently to tolerant behaviour, no error surfaced
- [ ] Tests: hover on template use, hover inside template body, hover on poly-width-dependent port, hover in a syntax-broken file (fallback), hover where the relevant template lives in an included file (single-file included closure only for MVP)

## Notes

Multi-file includes with the full include graph come for free once ticket 0418's loader closure is in place: expansion happens over the merged `File`, and span provenance carries `SourceId` back to the originating file.

Deferred to follow-up:

- Inlay hints for poly width and indexed port ranges — same underlying data, different LSP endpoint
- Peek expansion (code action showing the expanded body)
- Semantic tokens / rename / references — orthogonal, covered by the "cheap wins" tier in the roadmap memory

Out of scope: signal-graph features (dead-code, unused output, cycle warnings, cable path) — require a separate pass over `FlatPatch` connections, tracked as a deeper tier.
