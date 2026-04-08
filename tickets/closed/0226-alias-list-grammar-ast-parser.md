---
id: "0226"
title: Grammar, AST, and parser for alias lists in shape args
priority: medium
created: 2026-03-30
---

## Summary

Allow a shape arg to supply an alias list in place of an integer scalar:

```text
module mix : Sum(channels: [drums, bass, guitar])
```

This specifies the arity (`3`) and simultaneously declares a named index map
(`drums→0`, `bass→1`, `guitar→2`) scoped to the `mix` instance. The alias map
is used in T-0227 to resolve bare-ident indices in port refs and param entries.

This ticket covers grammar, AST, and parser only. The expander wiring (alias
map construction and alias resolution) is in T-0227.

## Acceptance criteria

- [ ] `grammar.pest` gains:
  - `alias_list = { "[" ~ (ident ~ ","?)* ~ "]" }`
  - `shape_arg` updated to `{ ident ~ ":" ~ (alias_list | scalar) }`.
- [ ] `ast.rs` gains `ShapeArgValue` enum:
  ```rust
  pub enum ShapeArgValue {
      Scalar(Scalar),
      AliasList(Vec<Ident>),
  }
  ```
  and `ShapeArg.value` field type changed from `Scalar` to `ShapeArgValue`.
- [ ] `grammar.pest`: `param_index` updated to accept bare `ident` as a third
      alternative alongside `nat` and `param_index_arity`:
      `param_index = { "[" ~ (param_index_arity | nat | ident) ~ "]" }`.
- [ ] `ast.rs`: `ParamIndex` gains `Alias(String)` variant.
- [ ] `parser.rs` builds `ShapeArgValue::AliasList` and `ParamIndex::Alias`
      correctly.
- [ ] Round-trip parse tests:
  - `channels: [drums, bass, guitar]` produces `ShapeArgValue::AliasList(["drums", "bass", "guitar"])`.
  - `gain[drums]: 0.8` produces `ParamIndex::Alias("drums")`.
  - Existing `channels: 3` and `gain[0]: 0.8` still parse as before.
- [ ] `cargo test` passes, `cargo clippy -- -D warnings` clean.

## Notes

Depends on T-0223.

Alias lists are valid only in shape-arg position. They are not valid in
parameter value position (e.g., you cannot write `value: [a, b, c]` as an
alias list — that parses as a `Value::Array` of bare-ident scalars, which is
a pre-existing form).

The `alias_list` rule uses `[]` brackets, same as array values, but appears
only in `shape_arg` where the grammar distinguishes it from `Value::Array` by
position.
