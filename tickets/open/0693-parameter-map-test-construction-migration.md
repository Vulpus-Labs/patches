---
id: "0693"
title: ParameterMap — migrate test construction off transitional API
priority: low
created: 2026-04-25
epic: E117
---

## Summary

Phase-2 follow-up to 0686. The redesign kept three transitional
methods on ParameterMap (`new`, `insert`, `insert_param`), all marked
`#[doc(hidden)]`, so existing test/bench callers would keep
compiling. Production paths no longer use them.

Migrate every call site to one of:
- `ParameterMap::default()` for empty,
- `[(name, idx, value), ...].into_iter().collect::<ParameterMap>()`
  for ad-hoc construction.

Then delete the three transitional methods.

## Acceptance criteria

- [ ] Zero call sites of `ParameterMap::new()`, `.insert(...)`, or
      `.insert_param(...)` outside of `parameter_map.rs` itself.
- [ ] The three methods removed from `parameter_map.rs`.
- [ ] `cargo test` passes.
- [ ] `cargo clippy` clean.
- [ ] Re-run `cargo mutants -p patches-core --file
      'patches-core/src/modules/parameter_map.rs'` — confirm no
      regression in catch ratio.

## Notes

Mechanical migration. ~230 call sites across tests/benches.
