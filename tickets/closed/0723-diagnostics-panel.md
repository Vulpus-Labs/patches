---
id: "0723"
title: Diagnostics panel rendering RenderedDiagnostic list
priority: medium
created: 2026-04-26
epic: "E122"
---

## Summary

Render the active `DiagnosticView` in the Diagnostics tab: severity
icon / colour, message, `file:line:col` location, and label.
Source-map projection lives in `patches-plugin-common::gui`.

## Acceptance criteria

- [ ] `DiagnosticSummary` (or successor type) projects severity,
      message, location, label from `RenderedDiagnostic` +
      `SourceMap`.
- [ ] Diagnostics tab lists each entry with colour by severity
      (error / warning / note).
- [ ] Empty state shown when there are no diagnostics.
- [ ] Clearing diagnostics on a successful compile clears the panel.
- [ ] `cargo clippy` and `cargo test` clean.
