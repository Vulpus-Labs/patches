---
id: "0617"
title: export_plugin! macro — emit all #[no_mangle] ABI symbols
priority: high
created: 2026-04-21
---

## Summary

A `macro_rules!` (or proc-macro if necessary) that takes a user
`Module` type and descriptor fn, emits all extern "C" entry
points the new ABI expects, and hides all `unsafe` from user
code.

## Acceptance criteria

- [ ] `export_plugin!(MyModule, my_descriptor)` expands to
      `#[no_mangle] pub extern "C" fn` entries for:
      `descriptor_hash`, `create`, `destroy`, `prepare`,
      `describe`, `update_validated_parameters`, `set_ports`,
      `process`.
- [ ] Generated `update_validated_parameters` body:
      `decode_param_frame` then forward to user's
      `Module::update_validated_parameters(&mut self, view)`.
- [ ] Generated `set_ports` body: `decode_port_frame` then
      forward.
- [ ] Lifetime of the plugin instance: generated `create`
      returns a `Box::into_raw`; `destroy` `Box::from_raw`.
- [ ] `cargo expand` inspection shows no dead unsafe blocks.
- [ ] Smoke test inside `patches-ffi-common` instantiates a
      trivial Module via the macro (no dylib build) and
      exercises every symbol.

## Notes

Epic E105. `macro_rules!` preferred if sufficient — avoids an
extra proc-macro crate for a fixed emission pattern.
