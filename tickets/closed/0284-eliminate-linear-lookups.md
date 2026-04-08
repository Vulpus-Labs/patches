---
id: "0284"
title: Eliminate O(n) lookups in server and analysis
priority: low
created: 2026-04-08
---

## Summary

Two places use linear scans where indexed lookups would be straightforward:

1. **`position_to_byte_offset`** (server) iterates every byte of the source to
   find the target line. `build_line_index` already computes a line-start table
   for diagnostics and hover, but it's rebuilt each time and not used here.
   Store the line index in `DocumentState` once and use binary search in
   `position_to_byte_offset`.

2. **`SemanticModel::get_descriptor`** (analysis) falls back to a linear scan
   of all descriptors when the name doesn't contain `::`. Build a secondary
   index (unscoped name → scoped key) during `analyse()` so the fallback is
   O(1).

## Acceptance criteria

- [ ] `DocumentState` stores a precomputed line index (`Vec<usize>`).
- [ ] `position_to_byte_offset` uses the stored line index with binary search
      instead of scanning bytes.
- [ ] The stored line index is also used by `to_lsp_diagnostics` and
      `compute_hover` (which currently rebuild their own).
- [ ] `SemanticModel` holds a secondary `HashMap<String, String>` mapping
      unscoped names to scoped keys (or equivalent), and `get_descriptor`
      uses it instead of iterating.
- [ ] `cargo test -p patches-lsp` and `cargo clippy -p patches-lsp` pass clean.
