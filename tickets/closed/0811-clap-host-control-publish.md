---
id: "0811"
title: CLAP parameter publish, ID stability, tombstone table
priority: high
created: 2026-05-04
epic: E135
depends_on: "0810,0809,0815,0816"
---

## Summary

CLAP plugin consumes the host control manifest and publishes
automatable parameters with stable IDs across patch reloads (ADR
0057 §6).

The pure name → CLAP-param-ID lifecycle logic — fresh / live /
tombstoned / reborn states, monotonic ID allocation, kind-change /
info-change diff, LRU eviction at the 64 cap — is already implemented
and unit-tested in `patches-plugin-common::host_control_registry`
behind the `HostControlRegistry` trait. This ticket integrates that
registry into the CLAP plugin: the plugin owns a
`StandardHostControlRegistry`, drives `apply()` on each manifest
update, dispatches the resulting `RescanLevel` to the host, and
hooks `record_value` into the audio-thread event handler.

Blocked on E136. The audio-thread event handler writes into the
host-control scratch buffer (ticket 0816), which doesn't exist until
that epic closes.

## Acceptance criteria

- [x] Plugin listens on the manifest ring; on each manifest update,
      reconciles published CLAP params:
      - Names already known: retain ID and current value.
      - New names: assign fresh IDs.
      - Removed names: ID retained in a session tombstone table; not
        reused.
- [x] CLAP `params` extension publishes name, range, default, label,
      taper, units interpreted from `HostControlParamMap`.
      `param_range` clamps malformed values (lo/hi swap, out-of-range
      default) silently — no panic. Field-level validation deferred
      to DSL validate (ticket 0807).
- [x] Per-block CLAP parameter event queue drives writes to the
      host-control backplane region. Sample-accurate via
      `HostControlEvent { sample_offset }` + `prepare_host_control_block`
      step-fill (ticket 0817), exceeds the ADR 0057 §4 collapse-per-
      block minimum.
- [x] Cross-session matching for host control values works when the
      DAW reopens a project. Persistence is via the name-keyed state
      stream (`extensions.rs::state_save` / `deserialize_state`).
      CLAP `cookie` is left null because slot/channel is plan-relative
      and a cached cookie would go stale on every replan; the state
      stream is the durable carrier.
- [x] Toggle kind publishes as a stepped/boolean CLAP param via
      `CLAP_PARAM_IS_STEPPED`; audio side still sees 0.0 / 1.0.
- [x] Tests: ID stability / tombstone preservation / reborn covered
      by `host_control_registry` unit tests
      (`add_preserves_existing_ids_and_shifts_slots`,
      `remove_moves_to_tombstone_with_id_preserved`,
      `reborn_reclaims_old_id_and_last_value`). State round-trip
      covered by `host_controls_section_round_trip` and
      `collect_host_controls_merges_registry_over_cache`.
- [x] `just inner -p patches-clap` passes.

## Notes

- Real-time/non-real-time boundary: CLAP `process()` parameter event
  queue runs on the audio thread; backplane writes from the audio
  side are acceptable since the audio thread already owns the read
  side. Confirm against ADR 0045/0046 plumbing direction — if writes
  must come from the control thread, route through the existing
  parameter-update channel instead.
