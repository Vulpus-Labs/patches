---
id: "0651"
title: 10k-cycle soak under allocator trap with randomised param updates
priority: high
created: 2026-04-23
epic: "E111"
depends_on: ["0649", "0650"]
---

## Summary

Integration soak test: 10 000+ process cycles with randomised
parameter updates across a representative patch (vintage bundle +
core modules). Under the Spike 4 allocator trap. Asserts no
audio-thread allocation and clean `Arc` cleanup at shutdown.

## Acceptance criteria

- [ ] Soak binary/test under `patches-integration-tests`.
- [ ] Allocator trap armed on audio thread; zero allocations across
      the run.
- [ ] Every `Arc<Library>` / `ArcTable` entry reaches refcount zero
      at shutdown; leak check green.
- [ ] Covers both in-process and bundle-loaded (patches-vintage)
      modules to exercise FFI frame paths.
- [ ] Runs in nightly CI; smoke variant (shorter cycle count) in
      PR CI.

## Notes

ADR 0045 §Spike 9. Depends on 0649/0650 so fuzz surface is locked
before the long-running soak.
