---
id: "0812"
title: Drop+replace and param-update fast path on manifest change
priority: medium
created: 2026-05-04
epic: E135
depends_on: "0809,0811"
---

## Summary

Distinguish shape changes (add / remove host control) from
metadata-only changes (rename, range, default, taper) and route each
through the right reload path.

## Acceptance criteria

- [x] Shape change → existing size-change drop+replace path for the
      `~host_control` module instance. CLAP republishes the
      parameter list.
- [x] Metadata-only change on an unchanged set → pure parameter
      update on the existing instance + CLAP param metadata
      republish. No audio-graph rebuild.
- [x] Detection compares prior manifest to new; equality on (sorted
      names) → fast path; else drop+replace.
- [x] Live-reload tests: rename without shape change preserves audio
      continuity (no zipper, no dropout); add new control rebuilds.
- [x] `just inner -p patches-engine -p patches-clap` passes.

## Resolution

Verification ticket — wiring already in place, no production code
changes needed.

- Shape change (add / remove host control): the synthesised
  `~host_control` module's `channels` axis tracks the manifest
  cardinality. The planner's existing size-change diff tombstones
  the old slot and installs a fresh instance whenever the channel
  count differs. CLAP-side, the registry diff promotes to
  `RescanLevel::All` (add / remove / kind change), which the
  CLAP host calls in `compile_and_push_plan`
  ([patches-clap/src/plugin.rs](../../patches-clap/src/plugin.rs)).
- Metadata-only change (range / default / taper / units, names
  unchanged): the synth module's `slot_offset[i]` / `kind[i]`
  arrays are positional and indifferent to the metadata payload, so
  the planner emits no parameter update for the synth instance.
  Registry promotes to `RescanLevel::Info`; CLAP republishes
  parameter info. No audio-graph rebuild.
- Rename within a fixed channel count: `slot_offset[i]` and
  `kind[i]` are keyed by alphabetical position, so an
  alias-rename that preserves cardinality leaves the synth's
  param frame byte-identical. The planner emits neither tombstone
  nor parameter update; audio continuity is preserved. CLAP-side
  the registry treats the rename as remove-old + add-new and
  promotes to `All`.

Tests added in
[patches-host/tests/host_control_reload.rs](../../patches-host/tests/host_control_reload.rs)
exercise all three paths by driving `compile_only` repeatedly
through a single `HostRuntime` and inspecting
`ExecutionPlan::tombstones` / `new_modules` against the
`MonitorMeta` slot table.

## Notes

- The shape-change path already exists for templates and taps; this
  is a wiring task, not new infrastructure.
