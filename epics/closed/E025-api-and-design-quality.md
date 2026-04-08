---
id: "E025"
title: API and design quality improvements
status: closed
priority: medium
created: 2026-03-20
tickets:
  - "0149"
  - "0150"
  - "0151"
  - "0152"
---

## Summary

Four medium-severity issues identified in the sniff-test review. None are blockers, but each imposes ongoing maintenance cost: unnecessary allocations on the control thread during every plan rebuild, a confusing three-headed ParameterMap lookup API, silent DSL defaults that mask typos, and a test harness that forces poly-module unit tests into slower integration-test territory.

## Tickets

- [T-0149](../tickets/open/0149-reduce-planner-parameter-cloning.md) — Reduce unnecessary cloning in planner parameter diff
- [T-0150](../tickets/open/0150-consolidate-parameter-map-api.md) — Consolidate ParameterMap lookup API
- [T-0151](../tickets/open/0151-dsl-warn-on-implicit-defaults.md) — Emit diagnostics for implicit DSL scale/index defaults
- [T-0152](../tickets/open/0152-harness-poly-support.md) — Extend `ModuleHarness` to support poly I/O
