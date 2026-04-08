---
id: "0132"
title: "`ParameterRef` in descriptors; DSL `name/index` syntax and tests"
priority: medium
created: 2026-03-18
epic: "E025"
depends_on: ["0131"]
---

## Summary

With `ParameterKey` and the `ParameterMap` newtype in place (T-0131), this
ticket completes the parity with ports by:

1. Adding `ParameterRef { name: &'static str, index: usize }` as the
   static-lifetime counterpart to `ParameterKey`, used in `ParameterDescriptor`
   and for lookup in descriptor-driven code.
2. Updating the YAML DSL parser to accept `"level/1"` as a parameter key
   meaning `name="level", index=1`. Bare `"level"` continues to mean index 0.
3. Adding tests that exercise multi-index parameters end-to-end.

## Acceptance criteria

- [ ] `patches-core/src/modules/module_descriptor.rs` defines:

  ```rust
  #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
  pub struct ParameterRef {
      pub name: &'static str,
      pub index: usize,
  }
  ```

  `ParameterDescriptor` is updated so its identification fields (`name`,
  `index`) are accessible via a `fn as_ref(&self) -> ParameterRef` method or
  equivalent, and `From<ParameterRef> for ParameterKey` is implemented.

- [ ] `patches-core/src/graph_yaml.rs` parses parameter map keys as follows:
  - `"level/0"` → `ParameterKey { name: "level", index: 0 }`
  - `"level/1"` → `ParameterKey { name: "level", index: 1 }`
  - `"cutoff"` (no slash) → `ParameterKey { name: "cutoff", index: 0 }`
  - A slash followed by a non-numeric or missing suffix is a
    `GraphYamlError::InvalidParameterKey` (new variant).

- [ ] Descriptor lookup in the YAML parser matches on both `name` and `index`:

  ```rust
  descriptor.parameters.iter().find(|p| p.name == key.name && p.index == key.index)
  ```

- [ ] Unit tests in `patches-core` (in `graph_yaml.rs` or a dedicated test
  module) cover:
  - A module descriptor with two parameters sharing a name (`"gain/0"`,
    `"gain/1"`); both can be set independently via YAML.
  - Bare `"gain"` sets `"gain/0"` and leaves `"gain/1"` at its default.
  - An unknown `"gain/2"` (not in the descriptor) yields
    `GraphYamlError::UnknownParameter`.
  - `"gain/abc"` yields `GraphYamlError::InvalidParameterKey`.

- [ ] `cargo test` passes across all crates. `cargo clippy` clean. No
  `unwrap()`/`expect()` in library code.

## Notes

`ParameterRef` uses `&'static str` (matching `PortRef`) because parameter
names come from compile-time module descriptors. `ParameterKey` uses `String`
because keys in a live map may originate from YAML parsing. The `From`
conversion bridges the two.

The test helper module for this ticket can define a minimal stub module with
two same-named parameters directly in the test file; it does not need to be
added to the module registry.
