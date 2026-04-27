---
id: "0729"
title: Rescan button triggers hard-stop reload
priority: medium
created: 2026-04-26
epic: "E123"
---

## Summary

Rescan button on the Modules tab kicks the hard-stop reload flow
(ADR 0044 §3) so newly-added scan directories take effect without
restarting the host.

## Acceptance criteria

- [ ] Button posts `Intent::Rescan`.
- [ ] `on_main_thread` runs the hard-stop reload flow already used
      by the existing rescan path.
- [ ] After rescan, modules from newly-added directories are
      available to the registry.
- [ ] Verified manually with a directory containing a test plugin
      (`test-plugins/gain` or similar).
- [ ] `cargo clippy` and `cargo test` clean.
