---
id: "0131"
title: "`ParameterKey` struct and `ParameterMap` newtype with zero-index aliases"
priority: high
created: 2026-03-18
epic: "E025"
---

## Summary

`ParameterMap` is currently a type alias for `HashMap<String, ParameterValue>`.
This ticket converts it to a newtype wrapping `HashMap<ParameterKey,
ParameterValue>`, where `ParameterKey { name: String, index: usize }` is the
new canonical key type. All existing call sites that use string keys continue to
compile unchanged: `get`, `insert`, and `contains_key` are re-exposed as
methods that default to index 0.

No module implementations and no DSL parsing change in this ticket; it is a
pure refactor with identical observable behaviour.

## Acceptance criteria

- [ ] `patches-core/src/modules/parameter_map.rs` defines:

  ```rust
  #[derive(Debug, Clone, PartialEq, Eq, Hash)]
  pub struct ParameterKey {
      pub name: String,
      pub index: usize,
  }

  impl ParameterKey {
      pub fn new(name: impl Into<String>, index: usize) -> Self { ... }
  }

  impl From<&str> for ParameterKey { /* index: 0 */ }
  impl From<String> for ParameterKey { /* index: 0 */ }
  impl fmt::Display for ParameterKey { /* "name" when index==0, "name/N" otherwise */ }
  ```

- [ ] `ParameterMap` is a newtype (`pub struct ParameterMap(HashMap<ParameterKey,
  ParameterValue>)`) with at minimum the following methods:

  ```rust
  // Zero-index aliases — existing call sites compile unchanged
  pub fn get(&self, name: &str) -> Option<&ParameterValue>;
  pub fn insert(&mut self, name: String, value: ParameterValue) -> Option<ParameterValue>;
  pub fn contains_key(&self, name: &str) -> bool;

  // Explicit-index access
  pub fn get_param(&self, name: &str, index: usize) -> Option<&ParameterValue>;
  pub fn insert_param(&mut self, name: impl Into<String>, index: usize, value: ParameterValue) -> Option<ParameterValue>;

  // Standard plumbing
  pub fn new() -> Self;
  pub fn is_empty(&self) -> bool;
  pub fn iter(&self) -> impl Iterator<Item = (&ParameterKey, &ParameterValue)>;
  ```

- [ ] `FromIterator<(ParameterKey, ParameterValue)>` is implemented so that
  `.collect::<ParameterMap>()` compiles in the planner diff code.

- [ ] `FromIterator<(String, ParameterValue)>` is implemented (index 0) so
  that any existing `.collect()` over string-keyed iterators continues to
  compile.

- [ ] All crates compile without modification to module implementations,
  planner, builder, engine, or integration tests beyond what is strictly
  required to satisfy the changed type.

- [ ] `cargo test` passes across all crates. `cargo clippy` clean.

## Notes

Only `patches-core/src/modules/parameter_map.rs` and (minimally) call sites
that iterate or collect over `ParameterMap` should need to change. The goal is
that anything calling `params.get("name")` or `params.insert("name".into(), v)`
requires zero edits.

The `iter()` return type exposes `&ParameterKey` rather than `&str`, so any
code that destructures the key will need updating — audit carefully. The
planner diff (`planner/mod.rs`) iterates over pairs and re-collects; update
those collect calls if needed.
