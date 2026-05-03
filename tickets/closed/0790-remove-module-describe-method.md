---
id: "0790"
title: Remove Module::describe trait method
priority: medium
created: 2026-05-02
epic: "E132"
adrs: ["0066"]
depends_on: ["0787", "0788", "0789"]
---

## Summary

With every module migrated to `const TEMPLATE`, remove
`Module::describe(shape)` from the trait. All call sites switch to
`Self::TEMPLATE.build_channels(channels)` directly, or to a registry
helper that holds templates by module name.

## Acceptance criteria

- [ ] `describe()` method deleted from `Module` trait.
- [ ] `Registry` exposes templates (or pre-built descriptors keyed by
      `(name, channels)`) without invoking a per-module function.
- [ ] All callers (`registry.describe(name, shape)` sites in
      planner, interpreter, FFI loader) updated.
- [ ] `cargo test --workspace` passes.
- [ ] `cargo clippy --workspace` clean.

## Notes

- Registry change shape: stores `&'static ModuleDescriptorTemplate`
  per registered module type instead of a `describe` fn pointer.
- This ticket is the "no going back" point — verify all migrations
  done before merging.

## Phase 1 (landed 2026-05-02): registry template-driven

- `ModuleBuilder` trait gained `fn template(&self) -> ModuleDescriptorTemplate`
  with `EMPTY` default; `Builder<T>` overrides via `T::template()`.
- `Registry` caches templates per registered name; `Registry::describe`
  builds from the cached template when non-empty, falling back to
  `builder.describe(shape)` for legacy paths (FFI dylibs and any
  not-yet-migrated built-in modules).
- `Registry::register::<T>` now keys on `T::template().name` when
  available, with a fallback to `T::describe(default_shape).module_name`
  for non-migrated types (e.g. `patches-vintage`).
- `Registry::template(name)` accessor added for callers that want the
  static template directly.

## Phase 2 (deferred): trait-method deletion

Blocked on:

- `patches-vintage` (12 modules) — needs its own migration ticket
  (no entry in the current epic ticket list).
- `patches-ffi-common` SDK macros + `test-plugins/*` — covered by
  E132 tickets 0795 / 0796 (FFI track).
- All `T::describe(&shape)` call sites across `patches-planner`,
  `patches-integration-tests`, `patches-svg`, `patches-lsp` — flip to
  `T::template().build_channels(...)` once vintage and FFI are done.

Phase 2 deletes `Module::describe`, drops `ModuleBuilder::describe`,
and removes the `templates.get(...).filter(...)` fallback in
`Registry::describe`.
