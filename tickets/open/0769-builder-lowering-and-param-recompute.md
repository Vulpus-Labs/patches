---
id: "0769"
title: Builder lowering of range cables and param-ref recompute
priority: medium
created: 2026-04-30
epics: ["E128"]
adrs: ["0062"]
---

## Summary

Wire the engine builder so each `FlatConnection`'s `CableMap`
reaches the destination input port: `scale`, `offset`, `clip` all
copied through at build time. Extend the existing port-update path
that today rewrites a single `scale` on parameter change so it
recomputes the full `(scale, offset, clip)` triple when any
endpoint or scalar segment was a `<param>` reference.

After this ticket, range cables are functional end-to-end.

## Acceptance criteria

- [ ] `patches-engine`'s builder copies `CableMap` fields to the
      destination port struct (`MonoInput`, `PolyInput`,
      `StereoInput`).
- [ ] Stereo broadcast (mono source → stereo input, ADR rule)
      preserves the affine+clip on both channels.
- [ ] Param-update path: when a connection had any `<param>`
      endpoint or scalar segment, recompute `CableMap` from current
      param values and rewrite the destination port. Touch only
      affected connections.
- [ ] Integration tests under `patches-integration-tests/`:
      - `uni(0.2, 0.8)` from a normalized source clips and offsets
        as expected.
      - `bi(C1, 2kHz)` driven by a `[-1, 1]` LFO produces v/oct at
        the destination matching ADR semantics.
      - `<param>`-driven endpoint updates the cable mid-run when the
        param changes.
      - Composition across a template boundary
        (`-[k]-> ... -[uni(lo, hi)]->` and the reverse) matches the
        algebra in 0767.
- [ ] `cargo test` and `cargo clippy` pass on the inner-loop
      subset and full workspace.

## Notes

Reference: [ADR 0062](../../adr/0062-cable-range-expressions.md).
Depends on 0767 (FlatConnection carries the triple) and 0768 (port
runtime applies it).
