---
id: "0715"
title: Wire Intent button stubs in new shell
priority: medium
created: 2026-04-26
epic: "E120"
---

## Summary

Hook each `Intent` variant up to a button or row affordance in the
new shell so end-to-end IPC round-trips can be verified before any
feature work. No dialogs, no rescan, no real reload — just confirm
the flag flips on `GuiState`.

## Acceptance criteria

- [ ] Buttons exist in the shell for: Browse, Reload, Rescan, Add
      Path. A placeholder remove-path button targets index 0.
- [ ] Each button posts the matching `Intent` JSON via
      `window.ipc.postMessage`.
- [ ] `on_main_thread` drains the corresponding flag and pushes a
      status-log line ("intent: rescan_requested") so the round-trip
      is observable.
- [ ] Removed: any reference to `Intent::PollMeter` or
      `meter_poll_requested`.
- [ ] `cargo clippy` and `cargo test` clean.

## Notes

Real picker / reload / rescan wiring lives in E123. This ticket only
proves the contract.
