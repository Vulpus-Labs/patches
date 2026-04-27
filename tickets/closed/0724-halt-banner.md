---
id: "0724"
title: Halt banner pinned at top of webview when engine halts
priority: medium
created: 2026-04-26
epic: "E122"
---

## Summary

When `GuiState::halt` is `Some(_)`, render a pinned banner at the
top of the webview describing the halt cause. Clears when the audio
thread reports the rebuilt engine is no longer halted (ADR 0051).

## Acceptance criteria

- [ ] Banner pinned above the tab strip; shown only when
      `halt.is_some()`.
- [ ] Format mirrors the TUI splash: module name, slot, first line
      of payload.
- [ ] Banner clears automatically once the audio callback reports no
      halt.
- [ ] Triggering a panic in a test module produces the banner; a
      successful reload clears it.
- [ ] `cargo clippy` and `cargo test` clean.
