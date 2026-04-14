---
id: "0412"
title: Migrate FlatPatch nodes from Span to Provenance
priority: medium
created: 2026-04-14
epic: E075
depends_on: ["0411"]
---

## Summary

Remove the plain `span: Span` field from `FlatModule`, `FlatConnection`,
`FlatPortRef`, `FlatPatternDef`, `FlatSongRow`, `FlatSongDef` in
`patches-dsl/src/flat.rs`. All consumers read `provenance.site`
instead.

## Acceptance criteria

- [ ] `span` field removed from all Flat node types in `flat.rs`.
- [ ] `patches-interpreter/src/lib.rs` consumers updated
      (~20 references) to read `provenance.site`.
- [ ] `patches-lsp/src/analysis.rs` diagnostic construction reads
      `provenance.site` (~3 sites).
- [ ] `patches-lsp` SVG rendering (`patches-lsp/src/server.rs:~294`)
      continues to work — its expansion use is unaffected but it
      should pass the new field through to any consumer that needs it.
- [ ] Any other workspace consumer updated (search for `flat_module.span`,
      `flat_conn.span`, etc.).
- [ ] `cargo build`, `cargo test`, `cargo clippy` clean.

## Notes

This is the irreversible step — once `span` is removed, every consumer
commits to the new shape. Low risk if 0411 left both fields green, as
the migration is a mechanical rename.

## Risks

- Silent semantic change if a consumer previously used `span` for
  something other than "point at my source". Grep for all usages and
  confirm each one wants `provenance.site` specifically (not, e.g., an
  outer call site).
