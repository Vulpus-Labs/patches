# ADR 0052 — Periodic update as a `Module` method

**Date:** 2026-04-24
**Status:** accepted

---

## Context

`Module::as_periodic() -> Option<&mut dyn PeriodicUpdate>` is listed as
footgun #1 in the v0.7.0 pre-release report. The contract is:

- Called exactly once at plan activation.
- Any impl must return `Some(self)` — never a temporary, never a borrow
  into a field.
- The returned reference has its lifetime erased to `'static` by
  `ModulePool::as_periodic_ptr` (`patches-engine/src/pool.rs:53-72`) and
  the raw pointer is cached in `ReadyState::periodic_modules`
  (`patches-engine/src/execution_state.rs:171`) for the life of the plan.

If an impl returns a field reference or a newtype wrapper by value, the
cached pointer dangles and is dereferenced every periodic tick. The
invariant is doc-only; `cargo clippy` cannot see it. All 15 current
in-tree impls happen to do the right thing (`Some(self)`) — the trait
variant exists only to cover a capability check the engine already
performs at plan build time.

The engine already materialises a `periodic_indices: Vec<usize>` into the
plan. The only information needed at that point is "does module `M` want
periodic updates?" — a question with a compile-time answer.

## Decision

Collapse `PeriodicUpdate` into `Module`:

```rust
pub trait Module {
    // ...existing methods...

    /// True if this module should receive `periodic_update` calls.
    /// Default: false. Evaluated at plan-build time, never per-tick.
    const WANTS_PERIODIC: bool = false;

    /// Called every `periodic_update_interval` samples for modules with
    /// `WANTS_PERIODIC == true`. Default: no-op.
    fn periodic_update(&mut self, _pool: &CablePool<'_>) {}
}
```

Delete `PeriodicUpdate` trait, `Module::as_periodic`, and
`ModulePool::as_periodic_ptr`. `ReadyState` keeps `periodic_slots:
Vec<usize>` and dispatches through normal `&mut dyn Module` each periodic
tick — no erased pointer, no cached `*mut dyn PeriodicUpdate`.

Plan build reads `WANTS_PERIODIC` at the construction site where the
concrete module type is still known (before erasure to `Box<dyn Module>`)
and pushes the slot index into `periodic_indices`.

FFI vtable (`FfiPluginVTable`, ADR 0045) gains a `wants_periodic: bool`
field and a `periodic_update` fn pointer. ABI_VERSION bumps (4 → 5).
`ModuleDescriptor` shape is unchanged, so descriptor-hash inputs are
unaffected.

## Consequences

**Positive:**

- Footgun #1 gone. Lifetime laundering disappears; no `transmute`, no
  raw pointer stored across ticks.
- One trait instead of two. Matches the capability-as-method pattern we
  already settled on when MIDI event handling was pulled into `Module`.
- `WANTS_PERIODIC` is a compile-time constant: zero runtime cost at plan
  build, branch-free dispatch at tick time (pre-filtered slot list).
- FFI vtable growth is additive for plugins; the `wants_periodic` bool
  is checked once at load, not per-tick.

**Negative:**

- ABI bump. Any out-of-tree v0.7.0 plugin must be rebuilt against
  ABI_VERSION = 5. Acceptable pre-1.0.
- `WANTS_PERIODIC` cannot vary with runtime config. No current in-tree
  module needs this — all 15 impls unconditionally return `Some(self)`
  and gate connectivity checks inside the update body. Future modules
  that want runtime gating do the same: set `WANTS_PERIODIC = true` and
  early-return from `periodic_update` when idle.

**Migration:** all in-tree impls replace
`fn as_periodic(&mut self) -> Option<&mut dyn PeriodicUpdate> { Some(self) }`
with `const WANTS_PERIODIC: bool = true;`. The existing
`impl PeriodicUpdate for M { fn periodic_update(...) { ... } }` block
becomes the inherent `periodic_update` method on `Module`. Mechanical.

## Alternatives considered

- **Keep the trait, enforce `Some(self)` with a `compile_fail` trybuild
  test.** Closes the footgun without an ABI bump but keeps two traits
  and the pointer-erasure machinery. Rejected: complexity for no gain
  once we accept the ABI bump is cheap pre-1.0.
- **`wants_periodic: bool` on `ModuleDescriptor`.** Shape change to
  `ModuleDescriptor` ripples through descriptor-hash inputs and every
  descriptor literal. Const on the trait is strictly cheaper and equally
  static.
- **`fn wants_periodic(&self) -> bool`.** Needs `&self`, implying a
  runtime-config story we don't have a use case for. Const is simpler.
