---
id: "E051"
title: "Cross-platform plugin GUI with vizia + baseview"
created: 2026-04-07
tickets: ["0274", "0275", "0276", "0277"]
---

## Summary

Replace the macOS-only AppKit GUI in `patches-clap` with a cross-platform
implementation using vizia and its built-in baseview backend. After this epic
the plugin has a working GUI on macOS, Windows, and Linux.

See ADR 0026 for the framework evaluation and decision rationale.

## Tickets

| ID   | Title                                              | Priority | Depends on |
|------|----------------------------------------------------|----------|------------|
| 0274 | Add vizia + baseview dependencies, scaffold module | high     |            |
| 0275 | Implement vizia UI (labels, buttons, state sync)   | high     | 0274       |
| 0276 | Integrate baseview with CLAP GUI lifecycle         | high     | 0275       |
| 0277 | Remove macOS-specific GUI code and dependencies    | medium   | 0276       |

## Definition of done

- The plugin GUI renders on macOS, Windows, and Linux inside a host-provided
  parent window.
- Browse and Reload buttons trigger file dialog and recompilation respectively.
- Path label and status label update to reflect current state.
- `gui_show()` and `gui_hide()` control window visibility.
- `gui_set_scale()` forwards DPI scale to vizia.
- All `objc2-*` dependencies and `gui_mac.rs` are removed.
- `cargo clippy` and `cargo test` pass with no warnings.
