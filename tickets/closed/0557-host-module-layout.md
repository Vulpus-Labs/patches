---
id: "0557"
title: Split HostRuntime into runtime.rs, fold callback.rs
priority: low
created: 2026-04-18
---

## Summary

`patches-host/src/builder.rs` mixes construction (`HostBuilder`) with
operational state (`HostRuntime`). `callback.rs` is a 30-line marker
trait (`HostAudioCallback`) that does not earn its own file. Split
runtime into `runtime.rs`; fold the callback trait into `lib.rs` (or
`runtime.rs` if it is only consumed alongside `HostRuntime`).

Part of epic E093.

## Acceptance criteria

- [ ] `patches-host/src/runtime.rs` holds `HostRuntime` and its impl.
- [ ] `patches-host/src/builder.rs` holds only `HostBuilder`.
- [ ] `callback.rs` deleted; `HostAudioCallback` lives wherever it is
      used from.
- [ ] Public re-exports from `lib.rs` unchanged (consumers should not
      need edits).

## Notes

Pure move; no behaviour change. Land after 0556 so the private-field
change sits in one place.
