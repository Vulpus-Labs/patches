---
id: "0421"
title: Migrate expansion-aware hover to PatchReferences
priority: medium
created: 2026-04-15
---

## Summary

With `PatchReferences` landed (ticket 0420), rewrite the expansion-aware
hover handlers to consult its precomputed tables instead of scanning
`FlatPatch` and walking `merged.templates`. Remove the ad-hoc helpers
`find_template_for_call_site`, `find_module_decl_type_name`,
`collect_port_wires`, and the inline smallest-enclosing call-site scan.

See ADR 0037 and epic E077.

## Acceptance criteria

- [ ] `compute_expansion_hover` takes `&PatchReferences` in place of
      `&SpanIndex` and the `&File` merged argument. `SourceMap` remains
      (still needed for `SourceId` resolution).
- [ ] Call-site hover uses `references.call_sites` to group expanded
      modules and `references.template_by_call_site` +
      `references.wires_by_template` to render the in/out port wiring
      section. No direct walk of `merged.templates` or
      `flat.modules.expansion`.
- [ ] Connection-span hover uses `references.connection_groups` to
      render fan-out targets. No re-filter of `flat.connections`.
- [ ] Definition-site module hover optionally uses
      `references.module_by_qname` where convenient (not required —
      `span_index.find_at` already returns the index).
- [ ] `workspace.rs` no longer passes `merged` into hover; the merged
      file stays cached only because `PatchReferences::build` consumes
      it. Drop the `merged_files` cache if nothing else needs it, or
      document why it stays.
- [ ] All existing hover tests pass unchanged (they assert markup
      content, not implementation paths).
- [ ] `cargo clippy` and `cargo test` clean for `patches-lsp`.

## Notes

Expected deletions in `patches-lsp/src/hover.rs`:

- `find_template_for_call_site`
- `find_module_decl_type_name`
- `collect_port_wires`
- `is_dollar_port`, `format_port_ref` (if template-wire formatting is
  lifted into `TemplateWires` at build time; otherwise keep them as
  render helpers against the precomputed structure)
- The call-site smallest-enclosing loop at the top of
  `hover_at_call_site`

The hover functions stay in `hover.rs` — they own the Markdown
formatting, just not the traversal.

Not covered here: inlay hints, peek expansion, completions, or any
other feature. Those arrive in separate epics and consume the same
`PatchReferences` without further structural change.
