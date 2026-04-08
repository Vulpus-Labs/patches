---
id: "0225"
title: Expander: resolve enum.member references to integers
priority: medium
created: 2026-03-30
---

## Summary

Teach Stage 2 expansion to resolve `Scalar::EnumRef` nodes to integer
literals. Given the `EnumDecl`s collected from the file, `drum.kick` resolves
to `Scalar::Int(0)`, `drum.snare` to `Scalar::Int(1)`, etc.

Enum references may appear anywhere a scalar is accepted: shape args, parameter
values, arrow scales, and port indices (as a sub-case of scalar use).

## Acceptance criteria

- [ ] The expander builds an enum environment `HashMap<String, HashMap<String, i64>>`
      from `File.enums` at the start of Stage 2 expansion.
- [ ] `Scalar::EnumRef { enum_name, member }` is resolved to `Scalar::Int(index)`.
      Unknown enum name or unknown member name → `ExpandError` with a helpful
      message including source span.
- [ ] Duplicate enum names → `ExpandError` (detected when building the env).
- [ ] `EnumRef` is valid in shape arg position, parameter value position, and
      arrow scale position.
- [ ] Tests:
  - Resolves `drum.kick`, `drum.snare`, `drum.hat` to `0`, `1`, `2`.
  - Unknown enum name → error.
  - Unknown member name → error.
  - Duplicate enum name → error.
  - `EnumRef` in shape arg: `Sum(channels: drum.hat)` expands to `channels: 2`.
- [ ] `cargo test` passes, `cargo clippy -- -D warnings` clean.

## Notes

Depends on T-0224.

Whether member names must be globally unique across enums is deferred to a
follow-up. For now, members are scoped to their declaring enum and referenced
via the two-part `enum.member` syntax, so global uniqueness is not required.
