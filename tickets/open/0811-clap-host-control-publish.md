---
id: "0811"
title: CLAP parameter publish, ID stability, tombstone table
priority: high
created: 2026-05-04
epic: E135
depends_on: "0810,0809"
---

## Summary

CLAP plugin consumes the host control manifest and publishes
automatable parameters with stable IDs across patch reloads (ADR
0057 §6).

## Acceptance criteria

- [ ] Plugin listens on the manifest ring; on each manifest update,
      reconciles published CLAP params:
      - Names already known: retain ID and current value.
      - New names: assign fresh IDs.
      - Removed names: ID retained in a session tombstone table; not
        reused.
- [ ] CLAP `params` extension publishes name, range, default, label,
      taper, units interpreted from `HostControlParamMap`. Malformed
      params produce a diagnostic at publish time, not a panic.
- [ ] Per-block CLAP parameter event queue drives writes to the
      host-control backplane region. Sample-accurate events collapse
      to one value per block per control (sub-block deferred per
      ADR 0057 §4).
- [ ] Cookie data carries the host control name for cross-session
      matching when the DAW reopens a project.
- [ ] Toggle kind publishes as a stepped/boolean CLAP param (host-
      side rendering hint); audio side still sees 0.0 / 1.0.
- [ ] Tests: ID stability across rename of unrelated control;
      tombstone preserved across remove; cookie round-trip.
- [ ] `just inner -p patches-clap` passes.

## Notes

- Real-time/non-real-time boundary: CLAP `process()` parameter event
  queue runs on the audio thread; backplane writes from the audio
  side are acceptable since the audio thread already owns the read
  side. Confirm against ADR 0045/0046 plumbing direction — if writes
  must come from the control thread, route through the existing
  parameter-update channel instead.
