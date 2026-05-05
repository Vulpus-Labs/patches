---
id: "0813"
title: Wire Controller settings to host-control manifest
priority: medium
created: 2026-05-04
epic: E135
depends_on: "0810"
---

## Summary

Replace the placeholder `host_controls: HashMap<String, f32>` on
`Controller` with a manifest-driven view: persisted values that don't
match a name in the current published manifest are dropped on ingress
with a status-log diagnostic.

LSP-side work originally bundled in this ticket is split out:

- 0822 — LSP structural diagnostics for host-control blocks
- 0823 — LSP hover for host-control declarations and references

## Acceptance criteria

- [x] `Controller.host_controls`
      ([patches-plugin-common/src/controller.rs](../../patches-plugin-common/src/controller.rs))
      is reconciled against `host_control_manifest` after every
      ingress (`StateLoad`, sidecar load, preset load, compile
      success). Names absent from the current manifest are dropped
      with a status-log diagnostic.
- [x] Reconcile defers when no manifest is available yet (the cache
      is held verbatim until the first compile lands one), so
      project reopen doesn't strip everything before the patch
      compiles.
- [x] `just inner -p patches-plugin-common` passes (62 → 64 tests).

## Notes

- The cache is retained (not removed) because it carries persisted
  values across CLAP `state_save` / `state_load` for entries that
  aren't currently live. The registry's tombstone table is in-memory
  only.
- LSP diagnostics for `HostControlUnknownRef`, missing required
  fields, name collisions, and template-scope blocks are tracked in
  ticket 0822. Hover surfaces are tracked in ticket 0823.
