---
id: "0662"
title: patches-clap halt error banner and silent passthrough
priority: medium
created: 2026-04-24
epic: E113
adr: 0051
depends_on: ["0660"]
---

## Summary

When the engine halts inside the CLAP plugin, keep feeding silence to the
host (so the DAW does not register a crash or dropout) and surface an
error banner in the Vizia GUI naming the offending module. Recovery is
via the existing rescan / patch reload path.

## Acceptance criteria

- [ ] CLAP `process` callback: on each block, after the engine tick loop,
      check `processor.halt_info()`. If halted, ensure the output buffer
      is zeroed (the engine already returns silence; this is belt-and-
      braces) and signal the GUI state that a halt is active.
- [ ] `GuiState` gains a `halt: Option<HaltInfoSnapshot>` field. Updates
      via the existing control-thread channel, not from the audio
      callback.
- [ ] Vizia view renders a top-of-window error banner when
      `halt.is_some()`, showing module name and payload first line.
      Banner has a "Reload patch" button that triggers the existing
      patch-load path.
- [ ] Rescan or patch reload clears the halt state and hides the banner.
- [ ] Manual test: load a patch containing a deliberately-panicking
      plugin module in a DAW (Bitwig, Reaper, or test-host); confirm
      (a) DAW does not crash, (b) audio goes silent, (c) banner shows
      correct module name, (d) reload recovers.

## Notes

Coordinate with the existing `gui_state` mutex cleanup work noted in the
pre-release report: once the `Mutex<GuiState>` expect()s are converted
to poison-tolerant accesses, the halt banner integration drops in
cleanly. If that work has not landed, add the halt field behind the
existing mutex and live with the poisoning risk for now.
