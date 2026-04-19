---
id: "0562"
title: Arc<Library> lifetime on DylibModuleBuilder and DylibModule
priority: high
created: 2026-04-19
epic: "E094"
---

## Summary

Ensure a loaded plugin `.dylib`/`.so`/`.dll` is never unmapped while
any builder or instance originating from it is still live. Every
`DylibModuleBuilder` and `DylibModule` holds a clone of
`Arc<libloading::Library>`; unload happens when the last clone drops.

This is the correctness precondition for runtime rescan/replace.

## Acceptance criteria

- [ ] `DylibModuleBuilder` stores `Arc<libloading::Library>` (or
      equivalent handle type wrapping it).
- [ ] `DylibModule` instances built by that builder clone the `Arc`
      and hold it for their lifetime.
- [ ] Registry replacement path drops the old `Arc` without calling
      `dlclose` explicitly; unload is solely via last-`Arc`-drop.
- [ ] Test: build two instances from one bundle, drop the builder,
      drop one instance; verify the second instance's vtable calls
      still succeed (library still mapped).
- [ ] Test: assert refcount > 1 when two builders from the same
      bundle coexist (extends 0496 if already in place).

## Notes

ADR 0044 §2. Works with the `Arc<libloading::Library>` already
introduced in E088 ticket 0496 — this ticket formalises the rule
for every builder and instance construction path, not just
bundle-sharing.
