---
id: "0773"
title: Re-key tap_opts from slot index to tap name
priority: medium
created: 2026-04-30
---

## Summary

`Controller::tap_opts` and `SerializedState::tap_opts` use `usize`
slot keys, which churn on patch edits. ADR 0063 §5 mandates name
keys. Migrate to `HashMap<String, TapDisplayOpts>` keyed by tap name
(from DSL `tap "foo"`).

## Acceptance criteria

- [ ] `tap_opts` keyed by `String` in Controller, snapshot, and
      serialized state.
- [ ] `Action::SetTapOpts` carries the tap name, not slot index.
- [ ] Both shells render and edit by name.
- [ ] Unnamed taps: opts are dropped on first encounter with a status
      log entry (suggested) or skipped silently — pick one and
      document in code.
- [ ] No external compatibility shim; in-tree CLAP state files are
      not yet in the wild.
- [ ] `cargo clippy` and `cargo test` pass.

## Notes

ADR 0063 §5. Blocks 0774 and 0776 (persistence shape).
