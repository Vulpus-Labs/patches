---
id: "0516"
title: New patches-host crate with shared player/CLAP composition
priority: medium
created: 2026-04-17
---

## Summary

Create `patches-host`, a crate that bundles the composition currently
duplicated between `patches-player` and `patches-clap`: registry
init, DSL pipeline driving, planner construction, plan-channel
wiring, processor spawn. Expose traits for the divergent bits so each
binary plugs in its own file source, audio callback structure, and
event handling.

Part of epic E089 (see ADR 0040). Depends on 0513 and 0515.

## Acceptance criteria

- [ ] New `patches-host/` crate exists with `publish = false`.
- [ ] Depends on `patches-core`, `patches-dsl`, `patches-interpreter`,
  `patches-registry`, `patches-planner`, `patches-engine`,
  `patches-diagnostics`.
- [ ] Exposes traits for divergent bits:
    - `HostFileSource` — path-based (player) vs in-memory string
      with optional include-base path (CLAP).
    - `HostAudioCallback` — pushes to engine (player) vs
      sample-accurate loop with transport/event extraction (CLAP).
    - `HostPatchSource` — error type + diagnostic bridging surface
      each host specialises.
- [ ] Exposes a `HostBuilder` (or similar) that produces
  `(Planner, Processor, plan_channel)` from a registry + sample rate.
- [ ] Exposes a shared patch-load helper that runs the full DSL
  pipeline (load → expand → bind → build → plan) and returns an
  ExecutionPlan or aggregated diagnostics.
- [ ] Unit tests cover the builder and the patch-load helper with a
  stub `HostFileSource`.
- [ ] `cargo build`, `cargo test`, `cargo clippy` clean.

## Notes

The trait shape will bend under the first two real consumers.
Expect to iterate after 0517 and 0518 land; the point of this ticket
is to give them a starting surface, not to freeze the API.

Research for the epic identified ~250+ lines of duplicated wiring
between `patches-player/src/main.rs` and
`patches-clap/src/plugin.rs` + `patches-clap/src/factory.rs`. Consult
those files when shaping the traits.

The CLAP path also has a cleanup thread and GUI state management
that are not shared with player; leave those in `patches-clap`.
