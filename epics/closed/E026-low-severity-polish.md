---
id: "E026"
title: Low-severity polish and code quality
status: closed
priority: low
created: 2026-03-20
tickets:
  - "0153"
  - "0154"
  - "0155"
  - "0156"
  - "0157"
  - "0158"
  - "0159"
---

## Summary

Seven low-severity issues from the sniff-test review: repeated string allocations from `NodeId(String)`, ~300 lines of near-identical `ModuleDescriptor` builder methods, missing `Copy` derives on small port types, redundant `ModuleDescriptor` clones in graph nodes, weak test assertions in the module registry test, a fix for the unused loop variable in ADSR tests, and missing documentation for the `CablePool` lifetime and `PortConnectivity` duplication.

## Tickets

- [T-0153](../tickets/open/0153-node-id-interning.md) — Intern `NodeId` strings via `Arc<str>`
- [T-0154](../tickets/open/0154-descriptor-builder-macro.md) — Collapse `ModuleDescriptor` builder repetition with a macro
- [T-0155](../tickets/open/0155-copy-derive-port-types.md) — Derive `Copy` for `MonoInput`, `MonoOutput`, `PolyInput`, `PolyOutput`
- [T-0156](../tickets/open/0156-arc-module-descriptor-in-graph.md) — Store `Arc<ModuleDescriptor>` in graph nodes to avoid repeated clones
- [T-0157](../tickets/open/0157-strengthen-registry-test.md) — Strengthen `default_registry_contains_all_modules` and fix ADSR unused variable
- [T-0158](../tickets/open/0158-document-cable-pool-lifetime.md) — Document `CablePool` lifetime and ping-pong mechanism
- [T-0159](../tickets/open/0159-document-port-connectivity-and-pool-capacity.md) — Document `PortConnectivity` duplication rationale and `DEFAULT_MODULE_POOL_CAPACITY`
