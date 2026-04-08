---
id: "E025"
title: Indexed parameter keys — name/index parity with ports
created: 2026-03-18
tickets: ["0131", "0132"]
---

## Summary

Inputs and outputs are identified by `(name, index)` pairs, so a mixer can
expose `"in"/0`, `"in"/1`, `"in"/2`. Parameters are currently identified by
name only (`HashMap<String, ParameterValue>`), forcing multi-channel modules to
encode the index into the name (`"level_0"`, `"level_1"`).

This epic introduces `ParameterKey { name: String, index: usize }` as the map
key for `ParameterMap`, and `ParameterRef { name: &'static str, index: usize }`
as the static descriptor counterpart — mirroring `PortRef`. All existing
call sites are preserved as zero-index aliases: `params.get("cutoff")` remains
valid and means `params.get_param("cutoff", 0)`. The DSL gains `name/index`
syntax for parameters that need explicit indices.

No existing module code changes are required.

## Tickets

| ID   | Title                                                              | Priority | Depends on |
|------|--------------------------------------------------------------------|----------|------------|
| 0131 | `ParameterKey` + `ParameterMap` newtype with zero-index aliases   | high     | —          |
| 0132 | `ParameterRef` in descriptors; DSL `name/index` syntax and tests  | medium   | 0131       |

## Definition of done

- `ParameterMap` is a newtype wrapping `HashMap<ParameterKey, ParameterValue>`.
- `params.get("name")` and `params.insert("name".to_string(), v)` continue to
  compile and behave as before (zero-index aliases).
- `params.get_param("name", n)` and `params.insert_param("name", n, v)` provide
  explicit-index access.
- `ParameterRef { name: &'static str, index: usize }` exists in
  `patches-core` and is used in `ParameterDescriptor`.
- The DSL accepts `"level/1"` as a parameter key meaning `("level", 1)`;
  bare `"level"` continues to mean `("level", 0)`.
- `cargo build`, `cargo test`, `cargo clippy` pass with no warnings across all
  crates.
- No `unwrap()` or `expect()` in library code.
