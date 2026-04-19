---
id: E093
title: patches-host API polish post-E089
created: 2026-04-18
status: open
---

## Summary

E089 extracted `patches-host` and ported both `patches-player` and
`patches-clap` onto it. The split is clean: consumers use the host API
consistently, `patches-cpal` is genuinely slimmed, and no
patch-building logic is duplicated. This epic captures the rough
edges found in post-E089 review — mostly API ergonomics inside
`patches-host` plus one small leak that forces both consumers to
re-derive source maps on the error path.

None of these are load-bearing; they are consumer-iteration
follow-ups of the kind the 0516 ticket explicitly anticipated.

## Goals

- Tighten `patches-host` public surface: privatise state, collapse
  `builder.rs` sprawl, drop dead file (`callback.rs`).
- Remove the one remaining abstraction leak where `CompileError`
  lacks a `SourceMap` and both consumers re-derive it.
- Clean up small residues left by the slim: unused dependency in
  `patches-cpal`, redundant `InMemorySource` work.

## Tickets

- 0556 — Privatise `HostRuntime` fields, add accessors.
- 0557 — Fold `callback.rs`, split runtime out of `builder.rs`.
- 0558 — Collapse `compile` + `push_plan`; drop tuple return.
- 0559 — Carry `SourceMap` on `CompileError`; remove consumer
  re-derivation.
- 0560 — Drop unused `patches-registry` dep from `patches-cpal`.
- 0561 — Cache canonical include root in `InMemorySource`.

## Non-goals

- No change to `HostAudioCallback` trait shape (deferred per 0516).
- No change to CLAP vtable / per-sample loop structure in
  `patches-clap` — the bulk there is legitimate CLAP protocol.
- No new host features; polish only.

## Notes

Review findings summarised in conversation 2026-04-18. The
`LoadedPatch` wrapper and `LayeringWarning` exposure were noted but
not ticketed — they are judgement calls that can wait for a second
consumer to pull them in a clear direction.
