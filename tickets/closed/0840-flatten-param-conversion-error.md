---
id: "0840"
title: Flatten `ParamConversionError` representation
priority: low
created: 2026-05-08
epic: E139
---

## Summary

[patches-interpreter/src/descriptor_bind/errors.rs:117-161](../../patches-interpreter/src/descriptor_bind/errors.rs#L117)
defines `ParamConversionError` as a three-variant enum where every variant
holds the same payload (`String`) and the variants exist purely to drive
`bind_code()` (which maps to `BindErrorCode`). Methods like
`prefix_with_param` re-construct the variant by hand to preserve the
discriminant, which is mechanical noise.

A struct-with-kind representation reads more cleanly and removes the
re-construction:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParamConversionKind { Unknown, TypeMismatch, OutOfRange }

#[derive(Debug, Clone)]
pub struct ParamConversionError {
    pub kind: ParamConversionKind,
    pub message: String,
}
```

`prefix_with_param` becomes a single `format!`; `bind_code` is a single
match on `kind`.

## Acceptance criteria

- [ ] `ParamConversionError` is a struct (or remains an enum with a clear
      reason recorded in the doc-comment after this ticket considers and
      rejects flattening)
- [ ] `prefix_with_param`, `message`, `into_message`, `bind_code`,
      `Display` all preserve current behaviour
- [ ] All call sites compile without changes to error-message wording
      (interpreter tests cover this — they assert exact strings)
- [ ] `just commit -p patches-interpreter` clean

## Notes

This is a representation change, not a logic change. Worth doing only if
the surrounding code is easier to read after; if the variant form turns
out to communicate intent better at call sites, close the ticket as
"considered, kept variant form" with a comment in `errors.rs` explaining
why.

Survey context: the original style report flagged this as "stringly-typed
error dispatch", but on closer reading the kind is already typed via the
variant discriminant. The remaining issue is just representational
ergonomics — hence `low` priority.
