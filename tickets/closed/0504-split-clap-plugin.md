---
id: "0504"
title: Split patches-clap plugin.rs
priority: medium
created: 2026-04-16
---

## Summary

`patches-clap/src/plugin.rs` is 678 lines covering the CLAP plugin
trait impl, parameter registration/marshalling, audio processing,
and MIDI/transport event handling.

## Acceptance criteria

- [ ] Convert to `plugin/mod.rs` with submodules:
      `params.rs` (CLAP parameter registration + get/set/flush),
      `audio.rs` (process callback body),
      `events.rs` (MIDI / transport event translation).
- [ ] Plugin struct + CLAP trait impls remain in `mod.rs`,
      delegating method bodies to submodule helpers.
- [ ] `mod.rs` under ~400 lines.
- [ ] `cargo build -p patches-clap`, `cargo clippy` clean.

## Notes

E086. No behaviour change. Skip if on closer inspection the file
does not factor cleanly along these axes — defer with a note back
to the epic rather than forcing an awkward split.

## Status: deferred

On inspection the proposed `params.rs / audio.rs / events.rs`
axis does not fit this file:

- There are no CLAP parameter callbacks (no get/set/flush surface
  to extract) — `params.rs` has nothing to host.
- MIDI + transport event handling is ~20 inline lines inside the
  `plugin_process` sample loop, not a separable unit — splitting
  would require restructuring the per-sample loop just to hand off
  a few branches.
- The natural axis here would be "vtable callbacks vs plugin-state
  methods", not the three concerns named in the ticket.

Per the ticket's own guidance, deferring rather than forcing an
awkward split. The file sits at 678 lines (sub-700) and is on the
E086 borderline list. Revisit if E086 rebaseline shows it still
worth attacking, or extract the vtable layer separately.
