---
id: E114
title: Periodic update as Module method
status: open
created: 2026-04-24
adr: 0052
---

## Goal

Replace `Module::as_periodic` / `PeriodicUpdate` trait with a
`const WANTS_PERIODIC: bool` and a default `periodic_update` method on
`Module`. Eliminates the stored-raw-pointer footgun (v0.7.0 report
footgun #1) and simplifies the module surface. See ADR 0052.

## Motivation

`as_periodic` must return `Some(self)`; any impl returning a temporary
dangles the cached pointer in `ReadyState`. The contract is doc-only.
All in-tree impls already unconditionally return `Some(self)` — the
trait adds nothing a compile-time bool can't express. Ship this before
v0.7.0 rather than freezing a surface we plan to change immediately.

## Tickets

- 0663 — patches-core: add `Module::WANTS_PERIODIC` + default
  `periodic_update`; keep `as_periodic` temporarily
- 0664 — patches-engine: collect `periodic_indices` via
  `WANTS_PERIODIC` at plan build; drop `as_periodic_ptr` and the
  `PtrArray<dyn PeriodicUpdate>` in `ReadyState`
- 0665 — patches-modules: migrate all 15 impls from
  `impl PeriodicUpdate` + `as_periodic` to inherent `periodic_update` +
  `WANTS_PERIODIC = true`
- 0666 — patches-core: delete `PeriodicUpdate` trait and
  `Module::as_periodic`; remove `PeriodicUpdate` from public re-exports
- 0667 — patches-ffi-common / patches-ffi: add `wants_periodic` and
  `periodic_update` to `FfiPluginVTable`; bump ABI_VERSION 4 → 5;
  update `export_plugin!` macro and SDK

## Done when

- `PeriodicUpdate` trait no longer exists in the workspace.
- `ReadyState` holds no raw `*mut dyn PeriodicUpdate`.
- All in-tree modules compile and pass tests with the new surface.
- FFI ABI_VERSION = 5; `test-plugins/` rebuilt and loads in
  `patches-player`.
- v0.7.0 report footgun #1 closed.
