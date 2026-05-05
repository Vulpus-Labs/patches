---
id: "0810"
title: HostControlManifest types and planner→observer ring
priority: high
created: 2026-05-04
epic: E135
depends_on: "0808"
---

## Summary

Define manifest types (ADR 0057 §5) and the planner→observer
transport carrying them, parallel to the tap manifest ring (ADR 0053
§6).

## Acceptance criteria

- [x] `HostControlDescriptor { slot, name, kind, params, source }`
      with `kind: HostControlKind { Knob, Slider, Toggle }` and
      `params: HostControlParamMap` (untyped k/v of literals).
- [x] `HostControlManifest = Vec<HostControlDescriptor>` sorted by
      slot.
- [x] Planner emits the manifest from the expander output and the
      synthesised `~host_control` shape.
- [x] Lock-free ring (or existing control transport) delivers
      manifest from planner thread to observer / CLAP plugin
      thread. Audio thread does not see it.
- [x] Parallel to tap manifest plumbing — share infrastructure
      where it exists, do not duplicate.
- [x] Provenance tag populated from declaration source span.
- [x] Tests: round-trip a manifest through the ring; alphabetical
      slot ordering matches the expander's.
- [x] `just inner -p patches-core -p patches-engine` passes.

## Notes

- Per-kind validation deferred to CLAP plugin (ADR 0057 §5). Do not
  schema-check the param map in core.
