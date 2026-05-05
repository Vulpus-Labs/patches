---
id: E136
title: Untagged cable pool and host-control scratch buffer (ADR 0068)
status: open
created: 2026-05-04
---

## Goal

Implement ADR 0068. Two linked engine-side changes that together let
host control automation be smoothed and shipped from a per-block
scratch buffer into the cable pool with a single per-sample `memcpy`:

1. Drop the `CableValue` enum tag. Cable slots become fixed-width
   `[f32; 16]`. Kind is known statically by reader and writer from
   the connection descriptor.
2. Pre-render CLAP host-control events into a SoA scratch, smooth
   knob / slider rows in place, transpose to an AoS frame, and copy
   into the four reserved control cables one sample at a time.

E136 unblocks the resumption of E135 at ticket 0811. Until E136
closes, host control values continue to use the placeholder
backplane-cell writes from ticket 0809.

## Scope

In:

- Cable pool refactor: `CableValue` enum → `[f32; 16]` slot. Adjust
  every `read_*` / `write_*` call site, FFI raw-parts API,
  `midi_io` sentinel encoding.
- Host control scratch buffer machinery: SoA fill, one-pole smoothing
  pass for knob / slider, AoS transpose, per-channel tail-state
  carried across blocks, per-sample `memcpy` into the cable pool's
  control region.
- ADR 0057 §4 amendment: sample-accurate automation is now in scope
  via the scratch buffer; remove the parked status.
- Test plugins (FFI) recompiled against the new raw-parts API.

Out:

- CLAP parameter event ingestion (id → channel resolution from CLAP
  events): handled in ticket 0811 once E136 closes.
- Per-control override of smoothing time constant.
- Sub-block smoothing with a different filter (one-pole at ~5 ms is
  the only option for now).

## Tickets

- 0815 — Untagged cable pool: drop `CableValue` enum, fixed `[f32; 16]`
  slot, adjust `read_*` / `write_*` and all consumers.
- 0816 — Host-control scratch buffer: SoA fill + one-pole smoothing +
  AoS transpose + per-sample backplane `memcpy`. Replaces the
  placeholder backplane-cell writes from 0809.

## Notes

- Closing E136 before E135 resumes keeps the registry (already
  implemented in 0811's scope, currently sitting as an
  unintegrated unit-tested module in `patches-plugin-common`) on
  ice until the audio side is ready to consume it.
- ADR 0068 documents the design decisions and ordering. ADR 0057's
  §4 ("sub-block automation parked") becomes obsolete once 0816
  lands; amend it as part of 0816's acceptance.
