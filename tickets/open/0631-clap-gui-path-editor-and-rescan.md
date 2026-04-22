---
id: "0631"
title: patches-clap GUI path editor + hard-stop rescan
priority: medium
created: 2026-04-22
epic: "E094"
depends_on: ["0566"]
---

## Summary

GUI half of the original 0566. With persisted module paths and
activate-time scan already in place (0566), add the in-plugin UI
for editing the path list and the "Rescan" button that performs a
full hard-stop reload.

## Acceptance criteria

- [ ] GUI exposes an editable list of module paths (add/remove).
      Changes update the persisted state but do not auto-rescan.
- [ ] GUI "Rescan" button triggers the hard-stop reload flow:
      1. Stop processing / deactivate audio;
      2. Drop current `ExecutionPlan` (releases instance
         `Arc<Library>`);
      3. Scan, update registry;
      4. Recompile the active patch source;
      5. Reactivate / resume processing.
- [ ] If recompilation fails post-rescan, surface the error in the
      GUI and leave the previous (last-good) state untouched — do
      not strand the user with a silent audio dropout.
- [ ] Integration test that performs a rescan while the plugin is
      active and asserts continuity of audio output and registry
      state.

## Notes

ADR 0044 §3, §5. Softer double-buffered hot-swap is explicitly
out of scope.
