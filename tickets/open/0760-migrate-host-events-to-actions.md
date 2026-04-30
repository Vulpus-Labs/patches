---
id: "0760"
title: Migrate Activate and StateLoad to Action::*
priority: medium
created: 2026-04-30
epic: E127
adrs: ["0061"]
---

## Summary

`activate` and `state_load` become thin shells that build an
`Action::Activate` / `Action::StateLoad(SerializedState)` and feed it
through `Controller::apply`. Registry rebuild and post-load compile
move into the controller, eliminating duplicated logic between
"first activate" and "rescan-driven activate".

## Acceptance criteria

- [ ] `Action::Activate` rebuilds the registry from controller state
      and recompiles `dsl_source` if non-empty.
- [ ] `Action::StateLoad(SerializedState)` replaces controller fields,
      then synthesises `Action::Activate` if already activated.
- [ ] No persistable mutation lives in `plugin_activate` /
      `state_load` outside the action call.
- [ ] Existing `state_load_plus_activate_scans_module_paths` test
      still passes (or its equivalent on the controller).

## Notes

Audio endpoints are still set up in `plugin_activate` itself — the
controller doesn't own the audio ring, just the model.
