---
id: "0767"
title: FlatConnection affine + clip and segment composition
priority: medium
created: 2026-04-30
epics: ["E128"]
adrs: ["0062"]
---

## Summary

Replace `FlatConnection.scale: f64` with an affine + optional clip:

```rust
pub struct CableMap {
    pub scale: f64,
    pub offset: f64,
    pub clip: Option<(f64, f64)>,   // sorted (min, max)
}
```

Update the expander's emit-time composition (`patches-dsl/src/expand/expander/emit.rs`)
to compose two adjacent segments per ADR 0062:

```text
gain   = s2.gain * s1.gain
offset = s2.gain * s1.offset + s2.offset
clip   = intersect(map_forward(s1.clip, s2), s2.clip)
```

Pure-scalar cables still produce `{ scale: k, offset: 0.0,
clip: None }` so downstream code can pattern-match the fast path.

## Acceptance criteria

- [x] New `CableMap` type lives next to `FlatConnection` (or in a
      dedicated module under `patches-dsl/src/`); replaces the bare
      `scale` field on `FlatConnection`.
- [x] `PortEntry` (in `patches-dsl/src/expand/connection.rs`) carries
      `CableMap` instead of `scale: f64`. Existing constructors keep
      a `scalar(k)` shortcut.
- [x] `eval_scale` returns `CableMap`. Range variants lower per ADR:
      - `uni(lo, hi)` → `scale = hi - lo`, `offset = lo`,
        `clip = Some((min(lo,hi), max(lo,hi)))`.
      - `bi(lo, hi)`  → `scale = (hi - lo) / 2`,
        `offset = (hi + lo) / 2`,
        `clip = Some((min(lo,hi), max(lo,hi)))`.
- [x] Composition helper covers all four shapes (scalar∘scalar,
      scalar∘range, range∘scalar, range∘range) including the
      forward-map of an inner clip through an outer affine, then
      intersection with the outer clip.
- [x] Interpreter `descriptor_bind/connections.rs` propagates the
      triple unchanged to the next stage (still no runtime effect
      from offset/clip — that lands in 0768).
- [x] Tests: composition algebra covers every shape, including an
      inverted range (`lo > hi`) and a degenerate `uni(k, k)`
      (clip becomes a single point, scale = 0).
- [x] `cargo test -p patches-dsl -p patches-interpreter` and
      `cargo clippy` pass.

## Notes

Reference: [ADR 0062 §Composition](../../adr/0062-cable-range-expressions.md).
This ticket is the algebra. Runtime application lives in 0768; the
core port struct change is the visible behavioural seam.
