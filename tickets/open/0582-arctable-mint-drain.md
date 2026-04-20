---
id: "0582"
title: ArcTable<T>: mint, pending_release SegQueue, control-thread drain
priority: high
created: 2026-04-19
---

## Summary

Wrap the refcount slot array (ticket 0581) in a generic
`ArcTable<T>` that owns the `Arc<T>` values on the control thread
and brokers audio-thread release through a lock-free queue. This
is the type that modules will eventually touch via the
`HostEnv::*_release` callbacks; here we build it in isolation.

## Acceptance criteria

- [ ] `ArcTable<T>` in `patches-ffi-common::arc_table` holding:
      - `entries: Mutex<HashMap<u64, Arc<T>>>` (control-thread
        only);
      - `refcount: RefcountTable`;
      - `pending_release: crossbeam_queue::SegQueue<u64>`.
- [ ] Control-thread `mint(value: Arc<T>) -> Result<u64,
      ArcTableError::Exhausted>` allocates a fresh slot via the
      refcount table, bumps a per-table generation counter,
      stores the `Arc` in `entries`, returns the packed id.
- [ ] Audio-thread `release(id)`: calls
      `RefcountTable::release(id)`; if it was the last reference,
      pushes the id onto `pending_release`. No locking, no
      allocation.
- [ ] Audio-thread `retain(id)`: delegates to
      `RefcountTable::retain`; documented as "not required under
      retain-by-default delivery" per ADR 0045 section 2 point
      3, but exposed for the host-side dispatch code.
- [ ] Control-thread `drain_released(&mut self)`: pops everything
      from `pending_release`, removes each id from `entries`
      (dropping the `Arc`), and calls `RefcountTable::remove` to
      free the slot. Called by the runtime periodically and at
      teardown.
- [ ] Teardown: `Drop for ArcTable<T>` drains first, then drops
      any remaining entries (a non-empty set at this point is a
      leak; log it via `tracing::warn!` with the id count).
- [ ] Unit tests: mint/release/drain round-trip, drop count
      matches mint count; `pending_release` empties cleanly;
      exhaustion returns `ArcTableError::Exhausted`.
- [ ] `cargo clippy -p patches-ffi-common` clean.

## Notes

Ask before adding `crossbeam-queue` to `Cargo.toml` (per project
CLAUDE.md). If declined, fall back to a hand-rolled
MPSC-appropriate queue — but the ADR assumes `SegQueue`-style
semantics and a single-audio-thread producer, so the stock
crossbeam type is the natural fit.

The `entries` `HashMap` is only ever touched on the control
thread; the `Mutex` guards against accidental cross-thread
access in tests and is uncontended in production. Do not expose
lock-acquiring methods from the audio-thread surface.
