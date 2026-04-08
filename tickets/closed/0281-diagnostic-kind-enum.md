---
id: "0281"
title: Add DiagnosticKind enum, replace string matching for severity
priority: medium
created: 2026-04-08
---

## Summary

Diagnostic severity is currently determined by string-matching on the message
text (`contains("unknown module type")`, `contains("dependency cycle")`). This
is fragile — changing a message silently changes severity. Add a typed
`DiagnosticKind` enum to `ast_builder::Diagnostic` and use it for severity
mapping.

## Acceptance criteria

- [ ] `Diagnostic` gains a `kind: DiagnosticKind` field.
- [ ] `DiagnosticKind` has at least: `SyntaxError`, `MissingToken`,
      `UnknownModuleType`, `DependencyCycle`, `UnknownPort`, `UnknownParameter`.
- [ ] All diagnostic construction sites in `ast_builder.rs` and `analysis.rs`
      set the appropriate kind.
- [ ] `to_lsp_diagnostics` maps severity from `DiagnosticKind` instead of
      string matching.
- [ ] Existing tests still pass; no change in observable behaviour.
- [ ] `cargo test -p patches-lsp` and `cargo clippy -p patches-lsp` pass clean.

## Notes

This also opens the door for future features like diagnostic codes and
code-action quick fixes keyed on kind.
