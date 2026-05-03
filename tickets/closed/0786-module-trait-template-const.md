---
id: "0786"
title: Module::TEMPLATE associated const with defaulted describe
priority: high
created: 2026-05-02
epic: "E132"
adrs: ["0066"]
depends_on: ["0785"]
---

## Summary

Add `const TEMPLATE: ModuleDescriptorTemplate` to the `Module` trait.
Provide a defaulted `describe(shape)` body that calls
`Self::TEMPLATE.build_channels(shape.channels as u32)`. Modules
overriding `describe()` continue to work; modules that have migrated
to a `TEMPLATE` const get the default.

## Acceptance criteria

- [ ] `Module::TEMPLATE` added in
      `patches-core/src/modules/module.rs`.
- [ ] Defaulted `describe()` returns
      `Self::TEMPLATE.build_channels(shape.channels)`.
- [ ] All existing modules compile unchanged (each provides a stub
      `const TEMPLATE = ModuleDescriptorTemplate::EMPTY`-style
      placeholder, or the trait sets a default empty template — pick
      whichever keeps the tree green; document choice in ticket
      close-out).
- [ ] `cargo build --workspace` clean.

## Notes

- The placeholder/empty default is temporary scaffolding — every
  migrated module replaces it with a real `TEMPLATE` and removes its
  `describe()` override. Migration tickets 0787-0789 do that work.
- Once all modules are migrated, ticket 0790 deletes the
  `describe()` method from the trait entirely.

## Close-out

Modeled as `fn template() -> ModuleDescriptorTemplate where Self: Sized`
rather than `const TEMPLATE` because `Module` is used as `dyn Module`
(harness, registry) and associated consts on a trait make it
dyn-incompatible — `where Self: Sized` clauses on consts are still
nightly-only (rust-lang/rust#113521). The `where Self: Sized` bound on
`template()` keeps the method out of the vtable, preserving dyn-compat.

Module impls still keep the canonical template as a `const TEMPLATE:
ModuleDescriptorTemplate = ...;` on the impl block; the trait method is
a one-line `fn template() -> _ { Self::TEMPLATE }` accessor. This
preserves the ADR 0066 source-of-truth-as-const intent without breaking
dyn dispatch.
