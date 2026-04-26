---
id: "0703"
title: Register AudioTap / TriggerTap in the module registry
priority: high
created: 2026-04-26
---

## Summary

Register the `AudioTap` and `TriggerTap` module types (built in
ticket 0699) with the default module registry so patches containing
tap declarations bind successfully. Closes the bind-broken state E118
ticket 0697 explicitly left for phase 2: the desugarer emits
synthetic instances with type names `AudioTap` / `TriggerTap`, but
those types weren't in the registry.

## Acceptance criteria

- [ ] `default_registry()` includes `AudioTap` and `TriggerTap`.
- [ ] An end-to-end load → expand → bind round-trip on a patch
  containing one or more tap targets succeeds with no
  `UnknownModule` error.
- [ ] Existing E118 desugar tests in `patches-dsl` continue to pass;
  add a complementary integration test in `patches-integration-tests`
  exercising the bind step.
- [ ] Synthetic module names (`~audio_tap`, `~trigger_tap`) are
  rendered correctly in any diagnostic that surfaces them — confirm
  the `~` prefix doesn't break existing diagnostic formatters.

## Notes

This ticket is small but has to land before any patches-player or
TUI work can run patches with taps. Treat it as a milestone that
flips taps from "parses, does nothing" to "parses, binds, runs".

## Cross-references

- E118 ticket 0697 — desugarer emitting these synthetic instances.
- Ticket 0699 — module implementations.
- E119 — parent epic.
