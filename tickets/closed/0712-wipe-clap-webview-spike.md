---
id: "0712"
title: Wipe CLAP webview spike artefacts
priority: high
created: 2026-04-26
epic: "E120"
---

## Summary

Strip the spike-era artefacts from the CLAP webview before rebuilding.
Plugin must still load with a blank webview after this ticket.

## Acceptance criteria

- [ ] `patches-clap/assets/hello.html` deleted.
- [ ] `applyMeter` IPC channel removed from `patches-clap/src/gui.rs`.
- [ ] `meter_poll_requested` field removed from
      `patches-plugin-common::GuiState` and all references.
- [ ] `Intent::PollMeter` variant removed.
- [ ] Spike-era fields trimmed from `GuiSnapshot` (kept fields and
      reshape are 0714's concern; just remove what the spike added).
- [ ] Plugin still loads in a CLAP host; webview opens to a blank
      page, no panics.
- [ ] `cargo clippy` and `cargo test` clean.

## Notes

This is a prep ticket. 0713 lands the new shell, 0714 reshapes the
snapshot.
