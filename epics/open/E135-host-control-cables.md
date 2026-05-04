---
id: E135
title: Host control cables (ADR 0057)
status: open
created: 2026-05-04
---

## Goal

Implement ADR 0057: a third input lane for DAW-side automation,
disjoint from patch parameters (ADR 0046) and MIDI (ADR 0048).
Patch authors declare `knob` / `slider` / `toggle` blocks at top-
level scope; the CLAP plugin publishes them as automatable parameters
with stable names; the audio side reads block-rate values via a
synthesised `~host_control` module that copies a backplane region to
its outputs.

Today only a placeholder exists: a `host_controls: HashMap<String, f32>`
field on `Controller` settings ([patches-plugin-common/src/controller.rs:34](patches-plugin-common/src/controller.rs#L34))
that is persisted but unused. No grammar, no module, no manifest,
no CLAP wiring.

## Scope

In:

- DSL grammar + expander for the block declaration form (Amendment
  2026-04-30) and bare-name reference resolution
- Synthesised `~host_control` module + `HostControl` runtime in
  patches-modules
- `HostControlDescriptor` / `HostControlManifest` types
- Planner→observer ring for the manifest, parallel to the tap
  manifest (ADR 0053 §6)
- Backplane region for host control, audio-side reads, control-side
  writes (Acquire/Release as ADR 0045/0046)
- CLAP plugin: parameter publish, ID stability + tombstone table,
  cookie-based cross-session matching by name
- Drop+replace on shape change; param-update fast path on rename /
  range / default change with unchanged set
- Wire the existing placeholder `host_controls` field through to the
  real manifest values, or remove it

Out (deferred):

- Sub-block / sample-accurate automation (ADR 0057 §4 explicitly
  parks this)
- New kinds beyond `knob` / `slider` / `toggle` (e.g. `xy_pad`)
- Persistence format beyond name-based cookie matching (future ADR
  per §6)
- Ratatui shell host-control surface (ADR 0063 covers the shell
  abstraction; this epic targets CLAP first)

## Tickets

- 0807 — DSL grammar + parser for host control blocks and bare-name
  references
- 0808 — Expander: collect blocks, synth `~host_control`, slot
  ordering, namespace resolution
- 0809 — `HostControl` module + backplane region plumbing
- 0810 — `HostControlManifest` types + planner→observer ring
- 0811 — CLAP parameter publish, ID stability, tombstone table
- 0812 — Drop+replace + param-update fast path on manifest change
- 0813 — Wire placeholder `host_controls` field to real manifest
  values; LSP diagnostics for name collisions

## Notes

- ADR 0057 supersedes its own §1 inline `~kind(...)` form with the
  block declaration form (Amendment 2026-04-30). Implement the block
  form; the inline form never shipped.
- `~` sigil is dropped on the source side. Stays for taps.
- No `.patches` files in tree use either form, so no migration.
- Host control name namespace is separate from module instances;
  cable identifier resolution: host control first, then module.
  Collision is a parse error naming both sites.
