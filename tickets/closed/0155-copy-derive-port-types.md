---
id: "0155"
title: Derive Copy for MonoInput, MonoOutput, PolyInput, PolyOutput
epic: E026
priority: low
created: 2026-03-20
---

## Summary

`MonoInput`, `MonoOutput`, `PolyInput`, and `PolyOutput` in `patches-core/src/cables.rs` are small structs (≤32 bytes, no heap data) that implement `Clone` but not `Copy`. Every function that takes one by value implicitly clones it via `Clone::clone`. Adding `Copy` makes moves explicit and eliminates the clone overhead at call sites.

## Acceptance criteria

- [ ] `#[derive(Copy)]` added to `MonoInput`, `MonoOutput`, `PolyInput`, `PolyOutput`.
- [ ] The compiler confirms all fields are `Copy` (they should be: `usize`, `f32`/`f64`, `bool`).
- [ ] No call sites need to be changed (adding `Copy` is always backwards-compatible for `Clone` users).
- [ ] `cargo clippy` clean; `cargo test` passes.
