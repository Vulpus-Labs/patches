---
id: "0158"
title: Document CablePool lifetime and ping-pong mechanism
epic: E026
priority: low
created: 2026-03-20
---

## Summary

`CablePool<'a>` in `patches-core/src/cable_pool.rs` uses a mutable borrow lifetime to enforce single-writer access to the ping-pong buffer. The mechanism is correct but non-obvious: a future maintainer unfamiliar with the design could misread the lifetime as incidental or attempt to refactor it away, inadvertently introducing a data race or write-index bug.

## Acceptance criteria

- [ ] A module-level or struct-level doc comment on `CablePool` explains:
  - What the ping-pong pool is and why it exists (1-sample cable delay, parallelism-readiness).
  - What `wi` (the write index) represents and why it flips between 0 and 1.
  - Why the `'a` lifetime on `&'a mut [[CableValue; 2]]` is load-bearing (prevents aliased mutable borrows).
- [ ] Each of `read_mono`, `read_poly`, `write_mono`, `write_poly` has a one-line doc comment clarifying which slot (read = `1-wi`, write = `wi`) it accesses.
- [ ] No code changes — documentation only.
- [ ] `cargo clippy` clean; `cargo test` passes.
