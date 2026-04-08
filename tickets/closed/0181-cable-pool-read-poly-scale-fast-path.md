---
id: "0181"
title: "`CablePool::read_poly`: skip 16-channel multiply when scale=1.0"
priority: medium
created: 2026-03-23
epic: "E031"
---

## Summary

`CablePool::read_poly` always applies `input.scale` to every channel of a poly
read via `channels.map(|v| v * input.scale)` ([cable_pool.rs:66](../../patches-core/src/cable_pool.rs#L66)).
This produces a new `[f32; 16]` array on every call.  The profiler shows
`read_poly` at **2.9% self time** on the audio thread (6.1% of actual compute
time), appearing as a leaf in 433 of ~7000 work-path samples.

The majority of connections in a typical patch use the default scale of 1.0
(only explicit `[scale]` annotations in the DSL produce a non-unity scale).
Multiplying 16 floats by 1.0 is wasteful; on Apple Silicon the compiler does not
eliminate it because `scale` is a runtime value.

Adding a branch for `scale == 1.0` allows the common case to return the cable
data without any arithmetic:

```rust
if input.scale == 1.0 {
    channels  // copy, no multiply
} else {
    channels.map(|v| v * input.scale)
}
```

## Acceptance criteria

- [ ] `CablePool::read_poly` is updated with a `scale == 1.0` fast-path that
  returns the raw channel array without a `map`.
- [ ] The comparison uses `== 1.0` (exact, not approximate) — this is safe
  because scale values are set from DSL-parsed literals or default constants,
  not accumulated arithmetic, so the value will be exactly `1.0_f32` when
  unscaled.
- [ ] All existing `CablePool` tests pass unchanged; `cargo test -p patches-core`
  green.
- [ ] `cargo clippy` passes with no new warnings.
- [ ] A micro-benchmark or comment documents the rationale for the exact
  comparison.

## Notes

`read_mono` has the same pattern but is not in the hot-path profile (its cost is
dwarfed by other work).  Applying the same fast-path to `read_mono` is
acceptable in this ticket but not required.

A future approach (not in scope here) would be to encode scale=1.0 as a field
flag or store it in a separate code path at `set_ports` time so no branch is
needed at all in `process`.  That requires a larger refactor of the port types
and is deferred.
