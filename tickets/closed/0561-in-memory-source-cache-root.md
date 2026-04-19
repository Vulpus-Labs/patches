---
id: "0561"
title: Cache canonical root in InMemorySource
priority: low
created: 2026-04-18
---

## Summary

`InMemorySource::load` (`patches-host/src/source.rs:96-113`)
re-canonicalizes and clones the master source on every include
resolution. Cache the canonical root once at construction; resolve
includes against the cached value.

Part of epic E093.

## Acceptance criteria

- [ ] Canonical root computed once, stored on `InMemorySource`.
- [ ] `load` no longer clones the master source per include.
- [ ] Existing host tests still pass; add a small test that a patch
      with several includes only triggers one canonicalize call (if
      cheaply observable — otherwise skip).

## Notes

Micro-optimisation; mostly a cleanliness fix. No user-visible
behaviour change.
