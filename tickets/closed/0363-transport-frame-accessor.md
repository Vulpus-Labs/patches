---
id: "0363"
title: TransportFrame accessor struct
priority: medium
created: 2026-04-12
---

## Summary

Introduce a `TransportFrame` struct in `patches-core` that provides
named, zero-cost accessors over the `GLOBAL_TRANSPORT` poly lane
layout (ADR 0031). This replaces bare lane-index constants with a
single-point-of-definition accessor layer, reducing index-coupling
and improving readability.

## Acceptance criteria

- [ ] `TransportFrame` struct in `patches-core` with associated
      constants matching existing `TRANSPORT_*` lane indices
- [ ] Typed read/write methods for each field (e.g.
      `playing(&[f32; 16]) -> bool`,
      `set_tempo(&mut [f32; 16], f32)`)
- [ ] Migrate `PatchProcessor::write_transport()` to use
      `TransportFrame` setters
- [ ] Migrate `PatchProcessor::tick()` sample counter write to
      use `TransportFrame`
- [ ] Migrate `HostTransport` module to use `TransportFrame`
      getters
- [ ] Migrate `MasterSequencer` host sync reads to use
      `TransportFrame` getters
- [ ] Migrate CLAP plugin transport writes to use
      `TransportFrame` setters
- [ ] Deprecate or remove bare `TRANSPORT_*` lane-index constants
- [ ] `cargo test` and `cargo clippy` pass

## Notes

- See ADR 0033 for design rationale.
- The underlying `[f32; 16]` representation is unchanged — this is
  purely a naming/access layer.
- Existing `TRANSPORT_*` constants can be kept temporarily as aliases
  if migration is staged, but should be removed once all call sites
  are updated.
