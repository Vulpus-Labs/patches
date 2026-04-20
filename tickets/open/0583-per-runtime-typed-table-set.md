---
id: "0583"
title: Per-runtime typed table set with caller-supplied capacity
priority: medium
created: 2026-04-19
---

## Summary

Expose a runtime-scoped container that owns one `ArcTable` per
payload type and ties mint/release to the typed id newtypes from
ticket 0580. Capacity is passed in at construction — the
planner-driven formula (ADR 0045 resolved design point 2) lands
in a later spike; for now the runtime supplies a value and the
test suite exercises exhaustion with a deliberately tight
budget.

## Acceptance criteria

- [ ] `RuntimeArcTables` (name negotiable) in
      `patches-ffi-common::arc_table` holding
      `ArcTable<[f32]>` (buffers) and `ArcTable<SongData>`
      (song/pattern data). `SongData` may be a stub struct in
      this ticket.
- [ ] Typed API: `mint_float_buffer(Arc<[f32]>) ->
      Result<FloatBufferId, ArcTableError>`,
      `release_float_buffer(FloatBufferId)`,
      and the `SongDataId` analogues. Untyped raw-u64 surface is
      crate-private.
- [ ] Capacities are separate per table, set via a
      `RuntimeArcTablesConfig { float_buffers: u32,
      song_data: u32 }`. Construction validates non-zero.
- [ ] Drain entry point `drain_released(&mut self)` fans out to
      each table.
- [ ] Integration test demonstrates that dropping the
      `RuntimeArcTables` drops every `Arc` it contains (mint a
      small number, leak-mint a couple, drop the container, use
      `Arc::strong_count` on the originals to confirm).
- [ ] `cargo clippy` clean across the workspace; the type is
      exported but no consumer outside `patches-ffi-common` is
      wired up yet (that is spike 3 onward).

## Notes

No `HostEnv` C vtable here — that is a spike 7 concern. The
typed methods are Rust-only for now; the ABI-facing release
callbacks will be thin shims over `release_float_buffer` /
`release_song_data` once the FFI ABI redesign lands.

Leave a `// TODO(ADR 0045 spike 6): grow` marker at the capacity
check site so the growth work has an obvious anchor point.
