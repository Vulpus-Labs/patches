---
id: "0813"
title: Wire Controller settings + LSP diagnostics
priority: medium
created: 2026-05-04
epic: E135
depends_on: "0810"
---

## Summary

Replace the placeholder `host_controls: HashMap<String, f32>` on
`Controller` with values driven from the real manifest, and surface
host-control-related diagnostics in the LSP.

## Acceptance criteria

- [ ] `Controller.host_controls` (in
      [patches-plugin-common/src/controller.rs](patches-plugin-common/src/controller.rs))
      sourced from the published manifest, not a free-form map.
      Persisted values match by control name; unknown names dropped
      on load with a diagnostic.
- [ ] LSP (`patches-lsp`) emits diagnostics for:
      - Block in template scope.
      - Missing required field (`low`/`high` for knob/slider,
        `default` for toggle).
      - Host control name colliding with module instance, citing
        both sites.
      - Bare-name reference to undeclared host control.
- [ ] Hover on a host control declaration shows kind + fields.
- [ ] Hover on a bare-name reference shows the linked declaration.
- [ ] `just inner -p patches-lsp -p patches-plugin-common` passes.

## Notes

- If the placeholder field has no remaining users after wiring,
  consider removing it instead. Decide during implementation;
  prefer removal over a half-used surface.
