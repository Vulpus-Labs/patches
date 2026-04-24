---
id: E113
title: Module panic halt policy
status: open
created: 2026-04-24
adr: 0051
---

## Goal

Make a panic from any module's `process()` or `periodic_update()` safe for
the host process: catch the unwind at the tick boundary, identify the
offending module, halt the engine to silence, and surface a diagnostic so
the user can reload. See ADR 0051.

## Motivation

FFI plugins (ADR 0045) can panic in module code compiled outside the host
binary. A Rust unwind across the `extern "C"` boundary is undefined
behaviour; in practice it crashes the DAW or `patches-player`. Native
modules are safer but not immune. The execution model offers no useful
"skip one module" recovery, so the right policy is clean halt + user-
triggered rebuild.

## Tickets

- 0658 — engine: attribution breadcrumb + halt flag on `ExecutionPlan`
- 0659 — engine: tick-level `catch_unwind` wrapper
- 0660 — engine: `Processor::halt_info()` query + integration tests
- 0661 — patches-player: halt diagnostic + reload prompt
- 0662 — patches-clap: halt error banner + silence passthrough

## Done when

- All tickets closed.
- Integration test: a module that panics in `process()` halts the engine
  within one tick, is named correctly in `halt_info()`, and subsequent
  ticks return silence without re-entering the module loop.
- Manual test: a panicking FFI plugin does not crash the CLAP host.
