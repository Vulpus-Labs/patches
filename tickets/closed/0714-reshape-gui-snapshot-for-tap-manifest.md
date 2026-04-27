---
id: "0714"
title: Reshape GuiSnapshot to project the tap manifest
priority: high
created: 2026-04-26
epic: "E120"
---

## Summary

Refit `GuiSnapshot` in `patches-plugin-common` so it carries the tap
manifest projection (per-tap name, slot, kind, components) needed by
the new shell. Bump `GuiSnapshot::VERSION`. Keep the existing throttle
and dedupe-by-json push machinery in `patches-clap/src/gui.rs`.

## Acceptance criteria

- [ ] `GuiSnapshot` carries a `taps: Vec<TapSummary>` field projected
      from the active manifest, ordered by slot. Each `TapSummary`
      includes `name`, `slot`, `kind` (string), and `components`.
- [ ] `GuiSnapshot::VERSION` bumped; old version constant removed.
- [ ] Snapshot serialisation tests updated, including a round-trip
      assertion for the new `taps` field.
- [ ] `applyState` JS stub receives the new shape (verified by
      reading `window.__patches.lastSnapshot` after a load).
- [ ] No tap *frame* data carried in `GuiSnapshot` — frames go via the
      separate `applyTaps` channel landed in 0716.
- [ ] `cargo clippy` and `cargo test` clean.

## Notes

Source the tap shape from the manifest types already used by the TUI
(`patches_dsl::manifest::Manifest`, `TapType`, etc).
