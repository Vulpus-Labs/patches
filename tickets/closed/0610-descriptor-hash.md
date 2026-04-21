---
id: "0610"
title: descriptor_hash — stable u64 over ModuleDescriptor
priority: high
created: 2026-04-21
---

## Summary

Host and plugin must agree on a hash of `ModuleDescriptor` to
detect ABI drift at load. Reuse or extend the `ParamLayout` hash
from Spike 1 (E097) if suitable; otherwise a dedicated descriptor
hash that covers port counts/names and parameter layout.

## Acceptance criteria

- [ ] `descriptor_hash(&ModuleDescriptor) -> u64` in
      `patches-ffi-common`, deterministic across runs and machines.
- [ ] Uses a stable serialiser (no `HashMap` iteration order,
      no pointer-derived values, no `Debug` formatting).
- [ ] Unit test: repeated computation for the same descriptor
      yields identical hash.
- [ ] Unit test: any single descriptor mutation (rename port,
      add parameter, change parameter kind, change enum
      variants) produces a different hash.
- [ ] Cross-process determinism test: spawn a helper binary that
      prints the hash for a known descriptor; compare to in-process
      computation. Skip-if-single-threaded-ci is fine.

## Notes

Epic E103. Consumed by E104 ticket 0614 (load-time check) and
E105 ticket 0617 (plugin exports its own via macro).
