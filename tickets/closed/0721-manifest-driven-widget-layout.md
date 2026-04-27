---
id: "0721"
title: Manifest-driven Patch tab layout
priority: high
created: 2026-04-26
epic: "E121"
---

## Summary

On `applyState`, populate the Patch tab with one widget per declared
tap, ordered by slot. Widget choice follows the tap kind (meter →
meter widget, osc → scope, spectrum → spectrum). Compound taps
render their components grouped under one label.

## Acceptance criteria

- [ ] Patch tab DOM is rebuilt when the snapshot's `taps` list
      changes; unchanged across frames otherwise.
- [ ] Widget instances are kept alive across `applyTaps` calls so
      canvases don't flicker.
- [ ] Widgets ordered by slot; tap name shown as label.
- [ ] Compound taps grouped: e.g. a `meter+osc` tap renders both
      widgets under one heading.
- [ ] Loading the existing TUI demo patches yields the same widget
      set as `patches-player`.
- [ ] `cargo clippy` and `cargo test` clean.

## Notes

This is the integration step that ties 0716–0720 together.
