---
id: "0581"
title: Lock-free refcount slot array with wait-free retain/release
priority: high
created: 2026-04-19
---

## Summary

Build the fixed-capacity, open-addressed slot array that backs
audio-thread retain/release in ADR 0045. Each slot is
`{ AtomicU64 id_and_gen, AtomicU32 refcount }`. Retain and release
on the audio thread are a single atomic `fetch_add` / `fetch_sub`
reached by direct slot indexing off the id. Linear probing is
confined to the control thread at insertion time.

## Acceptance criteria

- [ ] `RefcountTable` type in `patches-ffi-common::arc_table`
      holding a `Box<[Slot]>` of fixed length set at construction.
- [ ] Control-thread `insert(id_and_gen: u64) -> Result<u32,
      Exhausted>` uses linear probing from `slot(id) %
      capacity` to find an empty slot, stores the id_and_gen with
      `Release`, sets refcount to 1.
- [ ] Control-thread `remove(slot: u32)` zeroes id_and_gen after
      refcount reaches zero (called by the drain, see 0582).
- [ ] Audio-thread `retain(id)`: load slot from id, assert
      (debug) id_and_gen matches, `fetch_add(1, Relaxed)` on the
      refcount. No probing.
- [ ] Audio-thread `release(id) -> bool` (returns "was last"):
      `fetch_sub(1, AcqRel)` on the refcount, returns true if the
      prior value was 1. No probing.
- [ ] `retain` and `release` are `#[inline]` and contain no
      branches beyond the debug assert; no allocation, no locks.
- [ ] Unit tests: retain/release balance, last-release signal,
      debug assert fires on a stale id (wrong generation) in
      debug, is absent in release.
- [ ] `cargo clippy -p patches-ffi-common` clean; Miri clean on
      the module's tests.

## Notes

The slot's `id_and_gen` is `AtomicU64` only so the drain can
observe clean generation bumps; the audio path reads it only in
debug builds. A future spike (ADR 0045 spike 6) replaces the
`Box<[Slot]>` with an `AtomicPtr<[Slot]>` and an RCU-style
quiescence pair — leave a comment at the boxed allocation
pointing at that plan so the substitution is obvious.
