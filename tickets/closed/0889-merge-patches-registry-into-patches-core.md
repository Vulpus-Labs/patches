---
id: "0889"
title: Merge patches-registry into patches-core
priority: medium
created: 2026-05-11
---

## Summary

Move `patches-registry` (registry.rs, module_builder.rs) into
`patches-core/src/registry/` as a submodule. Delete the
`patches-registry` workspace member. Update every `use patches_registry::*`
import workspace-wide.

ADR 0040 carved `patches-registry` out of `patches-core` so the
"kernel" could be registry-agnostic. In practice every consumer pulls
both — the hygiene exists on paper and costs a published crate slot.
[ADR 0073](../../adr/0073-monorepo-split-into-successor-repos.md)
reverses the split to land on a 3-crate publication target
(patches-sdk, patches-core, patches-ffi-common).

## Acceptance criteria

- [x] `patches-core/src/registry/` created with the contents of
      `patches-registry/src/` (registry.rs, module_builder.rs).
      Module re-exports `Builder`, `ModuleBuilder`, `Registry`,
      `RegisterOutcome` at `patches_core::registry::*`.
- [x] `patches-registry/` workspace member removed; entry deleted
      from root `Cargo.toml` workspace members.
- [x] Every `use patches_registry::*` rewritten to
      `use patches_core::registry::*` across the workspace.
- [x] Every `patches-registry = { path = ... }` removed from
      consuming crates' Cargo.toml (now transitive via patches-core).
- [x] `patches-core` version bump 0.6.x → 0.7.0 (breaking change).
- [x] `cargo build --workspace`, `cargo test --workspace`,
      `cargo clippy --workspace` green.
- [x] Forbidden-edge lint updated: drop any rule mentioning
      patches-registry.
- [x] [ADR 0040](../../adr/0040-kernel-carve.md) gets a status note
      pointing at ADR 0073: "registry portion superseded; the rest
      (planner, cpal, host carves) still applies".

## Notes

Sequencing: do early in Phase A of [E146](../../epics/open/E146-monorepo-split.md).
Other Phase A tickets benefit from the simpler import surface
(patches-sdk no longer needs to re-export from two crates;
patches-manifest, patches-interpreter, etc. lose a Cargo dep line).

Compile-time cost: `patches-core` grows by ~400 LOC. Acceptable. The
"registry-agnostic core" goal was abstract; no consumer of core
omitted registry in practice.

Mechanical aspects:

- Most rewrites are `use patches_registry::Registry;` →
  `use patches_core::registry::Registry;`. `sed -i` job covers most
  of it.
- A few callers reference both crates explicitly today; collapse
  those.
- Doctests using `patches_registry::*` likewise rewritten.

Out of scope:

- Restructuring the registry module surface (rename methods, change
  signatures). Pure relocation.
- Touching `patches-ffi` (loader) which is separate from
  patches-registry — it stays as its own crate.

## Implementation notes

- New layout: `patches-core/src/registry/{mod.rs, registry.rs,
  module_builder.rs}`. Inner module marked
  `#[allow(clippy::module_inception)]` to keep the two-file split
  without the lint warning.
- Forbidden-edge rule `patches-svg -> patches-registry` removed; the
  registry edge that rule policed no longer exists as a separate
  crate. `patches-svg` already depends on `patches-core` directly via
  manifest plumbing.
- Re-export at `patches_core::registry` keeps imports stable shape —
  one path-prefix swap per file.
