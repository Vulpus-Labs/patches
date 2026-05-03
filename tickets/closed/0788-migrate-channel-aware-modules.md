---
id: "0788"
title: Migrate channel-aware modules to TEMPLATE const
priority: high
created: 2026-05-02
epic: "E132"
adrs: ["0066"]
depends_on: ["0786"]
---

## Summary

Migrate the 7 channel-aware modules — `sum`, `poly_sum`, `delay`,
`stereo_delay`, `tap`, `quant`, `poly_quant` — to declare
`const TEMPLATE` with explicit `global_*` and `per_axis_*` port/param
groups for the `channels` axis.

## Acceptance criteria

- [ ] Each module declares `const TEMPLATE` with the global vs
      per-channel split made explicit.
- [ ] Existing `describe()` override removed.
- [ ] Descriptor output byte-identical pre/post migration for a
      representative range of channel counts (1, 2, 8, 16) — add
      assertion to module tests.
- [ ] `cargo test -p patches-modules` passes.
- [ ] Multi-tap delay and stereo_delay behaviour unchanged in
      integration tests.

## Notes

- These modules currently call `mono_in_multi(name, channels)` etc.
  The template encodes the same intent declaratively.
- `delay` is the most complex (per-tap inputs + per-tap params);
  treat as the worst case for sizing this ticket.
