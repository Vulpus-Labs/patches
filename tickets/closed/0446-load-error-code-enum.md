---
id: "0446"
title: LoadErrorCode enum and typed load codes
priority: low
created: 2026-04-15
---

## Summary

`patches-diagnostics/src/lib.rs:104-109` hard-codes load-error code
strings (`"LD0001"`, …) in match arms inside `from_load_error`. Bind
errors, by contrast, have a proper `BindErrorCode` enum with an
`as_str()` method — a typed vocabulary consumers can reference.

Add `LoadErrorCode` with the same shape (`as_str`, `label`), rework
`from_load_error` to match on `LoadErrorKind` and map to a variant,
then call `as_str()`. Document the code registry in the crate-level
comment so future codes land in one place.

## Acceptance criteria

- [ ] `LoadErrorCode` enum exists with `as_str()` and `label()`.
- [ ] No hard-coded `"LD####"` string literals remain in
      `patches-diagnostics` outside the enum.
- [ ] `from_load_error` matches on `LoadErrorKind` variants, not
      rendered substrings.
- [ ] Existing diagnostic-output tests produce unchanged strings.
- [ ] `cargo test -p patches-diagnostics`, `cargo clippy` clean.

## Notes

Part of E082. Pairs naturally with 0441 (typed param errors); both are
eliminating stringly-typed classification.
