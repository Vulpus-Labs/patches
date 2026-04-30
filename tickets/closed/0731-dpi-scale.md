---
id: "0731"
title: Respect CLAP DPI scale arg in create_gui
priority: low
created: 2026-04-26
epic: "E124"
---

## Summary

`create_gui` currently ignores its `_scale` parameter. Wire it
through so HiDPI displays render crisply.

## Acceptance criteria

- [ ] `_scale` parameter consumed in `patches-clap/src/gui.rs`.
- [ ] Webview `LogicalSize` / `LogicalPosition` accounts for scale.
- [ ] Verified visually on a HiDPI macOS display.
- [ ] `cargo clippy` and `cargo test` clean.
