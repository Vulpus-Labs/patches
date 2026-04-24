---
id: "0659"
title: Tick-level catch_unwind in ExecutionPlan::tick
priority: high
created: 2026-04-24
epic: E113
adr: 0051
depends_on: ["0658"]
---

## Summary

Wrap `ExecutionPlan::tick()` in `std::panic::catch_unwind` so a module
panic is caught at the tick boundary rather than propagating through FFI
into the host. Use the breadcrumb from 0658 to identify the culprit.

## Acceptance criteria

- [ ] `tick()` body wrapped in `catch_unwind(AssertUnwindSafe(...))`.
- [ ] On `Err(payload)`: read `current_module_slot`; look up the slot's
      module name from the plan's slot metadata; fill `HaltInfo` with
      slot index, name, and a short stringified panic payload (truncate
      long payloads to 256 bytes); set `halted = true`; zero the tick's
      output buffers; return.
- [ ] If `halted` is already set on entry, `tick()` short-circuits to
      silence without entering the module loop or the `catch_unwind`
      wrapper.
- [ ] Integration test in `patches-integration-tests`: a harness module
      that panics in `process()` causes the engine to halt within one
      tick; subsequent ticks return silence; `halt_info()` names the
      correct module.
- [ ] Same test variant with the panic in `periodic_update()`.
- [ ] `panic = "unwind"` is documented in CLAUDE.md as a required profile
      setting for plugin crates and the CLAP host.

## Notes

`AssertUnwindSafe` is required because `&mut ExecutionPlan` is not
`UnwindSafe`. ADR 0051 explains why the torn mid-tick state is never
observed: halt is sticky, and rebuild is the only clear.
