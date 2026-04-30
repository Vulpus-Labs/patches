---
id: "0758"
title: Move dsl_source and module_paths ownership into Controller
priority: medium
created: 2026-04-30
epic: E127
adrs: ["0061"]
---

## Summary

Make `Controller` the canonical owner of `dsl_source` and
`module_paths`. `PatchesClapPlugin` keeps a `Controller` field and
delegates accessors. `GuiState.module_paths` mirror retained for now
(removed in 0761).

## Acceptance criteria

- [ ] `PatchesClapPlugin` holds `controller: Controller`.
- [ ] All reads/writes of `dsl_source` and `module_paths` route through
      the controller (or delegating accessors on the plugin).
- [ ] `state_save` / `state_load` read/write controller fields.
- [ ] `GuiSnapshot::from_state` reads paths from the controller via
      a temporary bridge (removed in 0761).
- [ ] Existing tests still pass; no behaviour change visible to host.

## Notes

This is a mechanical lift. Keep the `Intent`/`*_requested` flow intact
— that migration is 0759.
