---
id: "0274"
title: Add vizia + baseview dependencies and scaffold GUI module
priority: high
created: 2026-04-07
---

## Summary

Add vizia (with its baseview backend) as a dependency of `patches-clap` and
create the skeleton `gui_vizia.rs` module that will replace `gui_mac.rs`. This
ticket sets up the build infrastructure without changing any runtime behaviour.

## Acceptance criteria

- [ ] `vizia` added as a git dependency in `patches-clap/Cargo.toml` with the
      baseview feature enabled.
- [ ] A new `gui_vizia.rs` module exists with a placeholder `ViziaGuiHandle`
      struct and stub `create` / `destroy` / `update` methods matching the
      interface expected by `extensions.rs`.
- [ ] The module compiles on macOS, Windows, and Linux (CI or local
      cross-compile check).
- [ ] Existing macOS GUI behaviour is unchanged — `gui_mac.rs` is still the
      active implementation.
- [ ] `cargo clippy -p patches-clap` passes with no warnings.

## Notes

- vizia bundles baseview internally; a separate baseview dependency should not
  be needed unless the CLAP parent-window embedding requires calling baseview
  APIs directly. Investigate during implementation.
- ADR 0026 records the framework decision.
