---
id: "0262"
title: "patches-core: repr(C) and FFI accessors"
priority: high
created: 2026-04-07
---

## Summary

Add the minimal patches-core changes needed to support FFI plugin loading.
These are small, additive changes with no behavioural impact on existing code.

## Acceptance criteria

- [ ] `CableValue` enum in `cables.rs` has `#[repr(C)]` attribute
- [ ] `CablePool` has `pub fn as_raw_parts_mut(&mut self) -> (*mut [CableValue; 2], usize, usize)` returning the pool pointer, length, and write index
- [ ] `Registry` has `pub fn register_builder(&mut self, name: String, builder: Box<dyn ModuleBuilder>)` for registering a pre-built `ModuleBuilder` without the generic `register::<T>()` path
- [ ] All existing tests pass
- [ ] `cargo clippy` clean

## Notes

- `#[repr(C)]` on `CableValue` fixes the discriminant + data layout so both
  host and plugin agree on memory layout. This is the foundation of the
  zero-cost `process()` hot path (ADR 0025).
- `as_raw_parts_mut` is the only way for the host-side `DylibModule::process`
  to extract the raw pointer for passing across the FFI boundary. The lifetime
  is tied to the `&mut self` borrow, so the borrow checker still enforces
  exclusive access.
- `register_builder` is a one-method addition. The existing `register::<T>()`
  internally creates a `Builder<T>` that implements `ModuleBuilder`; the new
  method accepts any `Box<dyn ModuleBuilder>` directly.

Epic: E052
ADR: 0025
