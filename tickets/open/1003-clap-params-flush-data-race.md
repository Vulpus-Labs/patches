---
id: "1003"
title: "CLAP params_flush host-control registry data race"
priority: medium
created: 2026-06-11
---

## Summary

When the host calls `params.flush` on the audio thread (plugin active but
not processing), `host_control_registry.record_value`
(`patches-clap/src/extensions.rs:810-853`) races with main-thread
mutations of the same registry from `on_main_thread`. The registry mirror
is read/written from both threads with no synchronisation. Surfaced by the
2026-06 RT-safety review (adjacent to ticket 0997, deliberately scoped out
of it).

## Acceptance criteria

- [ ] No unsynchronised cross-thread access to the host-control registry
      mirror from `params_flush`.
- [ ] Either move the registry mirror onto an SPSC channel drained by the
      main thread, or make `last_value` an `AtomicU32` bit-cast of the
      `f32` (the lighter fix the review suggested as likely sufficient).
- [ ] The `TODO(0825?)` comment in `extensions.rs` removed once fixed.

## Notes

Replaces the dangling `TODO(0825?)` reference (ticket 0825 never existed).
Ticket 0997 noted: "an `AtomicU32` bit-cast f32 for `last_value` is likely
sufficient."
