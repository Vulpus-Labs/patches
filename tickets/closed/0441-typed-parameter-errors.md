---
id: "0441"
title: Typed parameter errors in descriptor_bind
priority: medium
created: 2026-04-15
---

## Summary

`patches-interpreter/src/descriptor_bind.rs:439-450` classifies parameter
errors by pattern-matching substrings of a rendered message (`"unknown
parameter"`, `"expected "`, `", found "`). This is fragile: any change
to the `convert_params` message format silently miscodes errors.

Return a typed error from `convert_params` (or wrap the existing error
in an enum at the call site) so `BindErrorCode` is chosen by matching
a variant, not a string.

## Acceptance criteria

- [ ] `convert_params` returns `Result<_, ParamConversionError>` where
      `ParamConversionError` is a concrete enum (Unknown, TypeMismatch,
      OutOfRange, …).
- [ ] `classify_param_error` is deleted.
- [ ] `BindErrorCode` selection is a `match` on the typed variant.
- [ ] Existing unit tests for parameter errors still pass; add one new
      test per variant asserting the correct `BindErrorCode` is chosen.
- [ ] `cargo test -p patches-interpreter`, `cargo clippy` clean.

## Notes

Part of E082. Low blast radius — `convert_params` lives inside
`descriptor_bind.rs`. Rendered messages can stay identical; only the
classification path changes.
