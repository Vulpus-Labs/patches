---
id: "0403"
title: Migrate LSP and SVG renderer to QName
priority: low
created: 2026-04-14
---

## Summary

Replace ad-hoc `(String, String)` pairs in `patches-lsp`
(`ScopeKey` in `analysis.rs`) and `patches-svg` with `QName` where the
value represents a qualified identifier.

## Acceptance criteria

- [ ] `patches-lsp` `ScopeKey` uses `QName`; lookups are path+name
      comparisons.
- [ ] `patches-svg` port-pair tuples either remain as
      `(QName, String)` (qualified module + port) or a clearly named
      struct, consistently applied.
- [ ] LSP and SVG tests/fixtures pass unchanged.

## Notes

Depends on 0402. Low risk; mostly type updates.
