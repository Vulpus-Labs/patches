---
id: "0822"
title: LSP structural diagnostics for host-control blocks
priority: medium
created: 2026-05-05
epic: E135
depends_on: "0810"
---

## Summary

Surface host-control-related structural diagnostics in `patches-lsp`
at edit time, mirroring the errors the expander already raises at
compile (ADR 0057, ticket 0808 / 0813).

## Acceptance criteria

- [x] LSP emits a diagnostic for a host-control block declared
      inside a `template { ... }` scope (`ST0032`).
- [x] LSP emits a diagnostic for a host-control block missing a
      required field: `low` / `high` for `knob` / `slider`,
      `default` for `toggle` (`ST0033`).
- [x] LSP emits a diagnostic when a host-control name collides
      with a module instance name (`ST0034`); the message names
      both sites.
- [x] LSP emits `ST0037` for a bare-name reference to an
      undeclared host control (mirrors the expander error).
- [x] Tests at
      [patches-lsp/src/workspace/tests/host_control.rs](../../patches-lsp/src/workspace/tests/host_control.rs)
      cover each diagnostic.
- [x] `just inner -p patches-lsp -p patches-plugin-common` passes
      (157 + 64 tests).

## Resolution

Verification ticket — wiring already in place.

`patches_dsl::validate::validate` (run as the first step of
`expand`) emits ST0032 / ST0033 / ST0034 / ST0036 for the four
in-template / missing-field / name-collision / duplicate-field
cases, and the host-control desugarer emits ST0037 for unresolved
bare-name references. The LSP's staged-pipeline runner
(`workspace::analysis::run_pipeline_locked`) already drives
`expand` and renders any `ExpandError` as a `RenderedDiagnostic`
carrying the `ST####` code; the workspace publish path surfaces
those alongside tolerant-AST diagnostics.

This ticket adds the test fixture pinning each code to its
violation so future grammar / validator edits can't silently
regress the LSP-visible surface.

## Notes

- The expander already produces `HostControlUnknownRef` and the
  desugarer enforces alphabetical-slot layout; some checks may
  reuse the structural pass via the LSP's diagnostic adapter
  rather than re-implementing them.
- Field-level validation (e.g. `low` < `high`) is out of scope —
  see CLAP plugin per ADR 0057 §5.
- Split out from ticket 0813. See ticket 0823 for hover.
