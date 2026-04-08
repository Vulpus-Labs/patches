---
id: "0245"
title: Migrate DSL tests to shared support module
priority: low
created: 2026-04-01
---

## Summary

The DSL test helpers (`parse_expand`, `module_ids`, `get_param`,
`connection_keys`) are duplicated across `expand_tests.rs` and
`torture_tests.rs`. A shared `tests/support/mod.rs` module now provides
canonical versions plus additional assertion helpers (`assert_modules_exist`,
`assert_connection_scale`, `find_module`, `find_connection`,
`parse_expand_err`).

## Acceptance criteria

- [ ] `tests/expand_tests.rs` — add `mod support;` import; remove local
      `parse_expand()`, `module_ids()`, `connection_keys()` definitions;
      use `support::*` versions. Where appropriate, replace manual
      `flat.modules.iter().find(...)` with `support::find_module()` and
      manual `flat.connections.iter().any(...)` with
      `support::find_connection()` or `support::assert_connection_scale()`.
- [ ] `tests/torture_tests.rs` — add `mod support;` import; remove local
      `parse_expand()`, `module_ids()`, `get_param()` definitions; use
      `support::*` versions. Replace repeated `ids.contains(...)` blocks
      with `support::assert_modules_exist()`.
- [ ] All tests pass, zero clippy warnings.

## Notes

Keep test-specific helpers (e.g. `parse_one_scalar` in `parser_tests.rs`)
local. Only migrate the helpers that are duplicated across files.

The `parser_tests.rs` file doesn't use `parse_expand` or `module_ids` and
doesn't need the support module — its consolidation is handled by T-0244.
